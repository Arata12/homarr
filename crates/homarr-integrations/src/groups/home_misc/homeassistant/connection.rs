//! Pooled Home Assistant websocket connection (`homeassistant-connection.ts`).
//!
//! One connection per integration id is shared by every request. A background task owns the socket: it
//! multiplexes commands, applies the compressed entity subscription to the state cache and broadcasts changes
//! to listeners. The connection closes itself when idle (no commands for five minutes and no listeners) or when
//! the socket closes; the pool then reconnects on the next use.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use homarr_common::{HomarrError, HomarrResult};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use super::types::{
    AreaEntry, CompressedStateEvent, DeviceEntry, EntityDisplayRegistry, FloorEntry,
    HomeAssistantRegistry, RawEntityState,
};
use crate::error::status_error;
use crate::groups::home_misc::ws::{self, Socket};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const STATES_INIT_TIMEOUT: Duration = Duration::from_secs(10);
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REGISTRY_TTL: Duration = Duration::from_secs(60);
const UNSUPPORTED_REGISTRY_CODES: [&str; 2] = ["unknown_command", "unauthorized"];

#[derive(Debug, Clone)]
pub enum ConnError {
    /// Home Assistant rejected the command (`HomeAssistantCommandError`).
    Command {
        code: String,
        message: String,
    },
    Closed,
    Timeout(String),
}

impl ConnError {
    pub fn into_homarr(self, ctx: &IntegrationContext) -> HomarrError {
        match self {
            Self::Command { code, message } => HomarrError::Upstream(format!(
                "{} rejected a command ({code}): {message}",
                ctx.name
            )),
            Self::Closed => {
                HomarrError::Upstream(format!("The connection to {} was closed", ctx.name))
            }
            Self::Timeout(what) => {
                HomarrError::Upstream(format!("{} did not answer in time ({what})", ctx.name))
            }
        }
    }

    fn is_code(&self, codes: &[&str]) -> bool {
        matches!(self, Self::Command { code, .. } if codes.contains(&code.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct StateChange {
    pub changed: Vec<RawEntityState>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Change(Arc<StateChange>),
    RegistryChanged,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Pending,
    Ready,
    Failed,
}

#[derive(Clone, Copy)]
enum SubKind {
    Entities,
    Registry,
}

type Reply = oneshot::Sender<Result<Value, ConnError>>;

enum Cmd {
    Send {
        message: Value,
        reply: Reply,
        sub: Option<SubKind>,
    },
    Close,
}

struct Shared {
    states: Mutex<HashMap<String, RawEntityState>>,
    phase: watch::Sender<Phase>,
    events: broadcast::Sender<StreamEvent>,
    registry_epoch: AtomicU64,
    closed: AtomicBool,
    last_used: Mutex<Instant>,
}

struct RegistryCache {
    fetched_at: Instant,
    epoch: u64,
    value: Option<HomeAssistantRegistry>,
}

pub struct HaConnection {
    url: Url,
    token: String,
    cmd: mpsc::UnboundedSender<Cmd>,
    shared: Arc<Shared>,
    registry: tokio::sync::Mutex<Option<RegistryCache>>,
    last_registry: Mutex<Option<HomeAssistantRegistry>>,
    registry_events: tokio::sync::Mutex<bool>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Opens the websocket and authenticates with the long-lived access token.
pub async fn open_authenticated(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    url: &Url,
    token: &str,
) -> HomarrResult<Socket> {
    let mut socket = ws::connect(rt, ctx, url, &[]).await?;
    socket
        .send(Message::text(
            json!({ "type": "auth", "access_token": token }).to_string(),
        ))
        .await
        .map_err(|err| {
            HomarrError::Upstream(format!("Failed to authenticate with {}: {err}", ctx.name))
        })?;
    let wait = async {
        while let Some(frame) = socket.next().await {
            let Ok(Message::Text(text)) = frame else {
                if matches!(frame, Ok(Message::Close(_)) | Err(_)) {
                    break;
                }
                continue;
            };
            let Ok(message) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            match message.get("type").and_then(Value::as_str) {
                Some("auth_ok") => return Ok(()),
                Some("auth_invalid") => {
                    return Err(status_error(ctx, reqwest::StatusCode::UNAUTHORIZED))
                }
                _ => {}
            }
        }
        Err(HomarrError::Upstream(format!(
            "The connection to {} was closed",
            ctx.name
        )))
    };
    match tokio::time::timeout(ws::CONNECT_TIMEOUT, wait).await {
        Ok(Ok(())) => Ok(socket),
        Ok(Err(err)) => {
            let _ = socket.close(None).await;
            Err(err)
        }
        Err(_) => {
            let _ = socket.close(None).await;
            Err(HomarrError::Upstream(format!(
                "Authenticating with {} timed out",
                ctx.name
            )))
        }
    }
}

fn state_from_compressed(
    entity_id: &str,
    compressed: &super::types::CompressedState,
) -> RawEntityState {
    let last_changed = compressed.lc * 1000.0;
    RawEntityState {
        entity_id: entity_id.to_string(),
        state: compressed.s.clone(),
        attributes: compressed.a.clone(),
        last_changed,
        last_updated: compressed.lu.map_or(last_changed, |lu| lu * 1000.0),
        context_id: compressed
            .c
            .as_ref()
            .and_then(super::types::CompressedContext::id),
    }
}

/// Applies a compressed `subscribe_entities` event (`applyCompressedEvent`).
pub fn apply_compressed_event(
    states: &mut HashMap<String, RawEntityState>,
    event: &CompressedStateEvent,
    replace: bool,
) -> StateChange {
    let mut changed: Vec<RawEntityState> = Vec::new();
    let mut removed = Vec::new();
    let push_changed = |changed: &mut Vec<RawEntityState>, state: RawEntityState| match changed
        .iter_mut()
        .find(|existing| existing.entity_id == state.entity_id)
    {
        Some(existing) => *existing = state,
        None => changed.push(state),
    };
    if replace {
        states.clear();
    }

    for (entity_id, compressed) in event.a.iter().flatten() {
        let state = state_from_compressed(entity_id, compressed);
        states.insert(entity_id.clone(), state.clone());
        push_changed(&mut changed, state);
    }

    for entity_id in event.r.iter().flatten() {
        if states.remove(entity_id).is_some() {
            removed.push(entity_id.clone());
        }
    }

    for (entity_id, diff) in event.c.iter().flatten() {
        let Some(current) = states.get(entity_id).cloned() else {
            continue;
        };
        let plus = diff.plus.as_ref();
        let mut attributes = current.attributes.clone();
        if let Some(added) = plus.and_then(|plus| plus.a.as_ref()) {
            for (key, value) in added {
                attributes.insert(key.clone(), value.clone());
            }
        }
        for key in diff
            .minus
            .as_ref()
            .and_then(|minus| minus.a.as_ref())
            .into_iter()
            .flatten()
        {
            attributes.remove(key);
        }
        let last_changed = plus
            .and_then(|plus| plus.lc)
            .map_or(current.last_changed, |lc| lc * 1000.0);
        let last_updated = plus
            .and_then(|plus| plus.lu.or(plus.lc))
            .map_or(current.last_updated, |value| value * 1000.0);
        let next = RawEntityState {
            entity_id: entity_id.clone(),
            state: plus
                .and_then(|plus| plus.s.clone())
                .unwrap_or_else(|| current.state.clone()),
            attributes,
            last_changed,
            last_updated,
            context_id: plus
                .and_then(|plus| plus.c.as_ref())
                .and_then(super::types::CompressedContext::id)
                .or(current.context_id.clone()),
        };
        states.insert(entity_id.clone(), next.clone());
        push_changed(&mut changed, next);
    }

    StateChange { changed, removed }
}

async fn run(mut socket: Socket, mut commands: mpsc::UnboundedReceiver<Cmd>, shared: Arc<Shared>) {
    let mut next_id: u64 = 1;
    let mut pending: HashMap<u64, Reply> = HashMap::new();
    let mut subscriptions: HashMap<u64, SubKind> = HashMap::new();
    let mut initialized = false;
    let mut tick = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(Cmd::Send { mut message, reply, sub }) = command else { break };
                let id = next_id;
                next_id += 1;
                message["id"] = json!(id);
                if let Some(kind) = sub {
                    subscriptions.insert(id, kind);
                }
                pending.insert(id, reply);
                if socket.send(Message::text(message.to_string())).await.is_err() {
                    break;
                }
            }
            frame = socket.next() => {
                let text = match frame {
                    Some(Ok(Message::Text(text))) => text.as_str().to_string(),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let Ok(parsed) = serde_json::from_str::<Value>(&text) else { continue };
                let messages = match parsed {
                    Value::Array(items) => items,
                    other => vec![other],
                };
                for message in messages {
                    handle_message(&message, &shared, &mut pending, &mut subscriptions, &mut initialized);
                }
            }
            _ = tick.tick() => {
                pending.retain(|_, reply| !reply.is_closed());
                let idle = lock(&shared.last_used).elapsed() > IDLE_TIMEOUT;
                if idle && shared.events.receiver_count() == 0 {
                    break;
                }
            }
        }
    }

    shared.closed.store(true, Ordering::SeqCst);
    for (_, reply) in pending.drain() {
        let _ = reply.send(Err(ConnError::Closed));
    }
    lock(&shared.states).clear();
    shared.phase.send_replace(Phase::Failed);
    let _ = shared.events.send(StreamEvent::Closed);
    let _ = socket.close(None).await;
}

fn handle_message(
    message: &Value,
    shared: &Shared,
    pending: &mut HashMap<u64, Reply>,
    subscriptions: &mut HashMap<u64, SubKind>,
    initialized: &mut bool,
) {
    let Some(id) = message.get("id").and_then(Value::as_u64) else {
        return;
    };
    match message.get("type").and_then(Value::as_str) {
        Some("event") => match subscriptions.get(&id) {
            Some(SubKind::Entities) => {
                let Some(event) = message.get("event").and_then(|event| {
                    serde_json::from_value::<CompressedStateEvent>(event.clone()).ok()
                }) else {
                    tracing::warn!("Ignoring malformed Home Assistant entity event");
                    return;
                };
                let change =
                    apply_compressed_event(&mut lock(&shared.states), &event, !*initialized);
                if !*initialized {
                    *initialized = true;
                    shared.phase.send_replace(Phase::Ready);
                }
                if !change.changed.is_empty() || !change.removed.is_empty() {
                    let _ = shared.events.send(StreamEvent::Change(Arc::new(change)));
                }
            }
            Some(SubKind::Registry) => {
                shared.registry_epoch.fetch_add(1, Ordering::SeqCst);
                let _ = shared.events.send(StreamEvent::RegistryChanged);
            }
            None => {}
        },
        Some("result") => {
            let Some(reply) = pending.remove(&id) else {
                return;
            };
            if message.get("success").and_then(Value::as_bool) == Some(true) {
                let _ = reply.send(Ok(message.get("result").cloned().unwrap_or(Value::Null)));
                return;
            }
            subscriptions.remove(&id);
            let error = message.get("error");
            let field = |name: &str| {
                error
                    .and_then(|error| error.get(name))
                    .and_then(Value::as_str)
            };
            let _ = reply.send(Err(ConnError::Command {
                code: field("code").unwrap_or("unknown_error").to_string(),
                message: field("message")
                    .unwrap_or("Home Assistant rejected the command")
                    .to_string(),
            }));
        }
        _ => {}
    }
}

impl HaConnection {
    fn start(socket: Socket, url: Url, token: String) -> Arc<Self> {
        let (cmd, commands) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let shared = Arc::new(Shared {
            states: Mutex::new(HashMap::new()),
            phase: watch::channel(Phase::Idle).0,
            events,
            registry_epoch: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            last_used: Mutex::new(Instant::now()),
        });
        tokio::spawn(run(socket, commands, shared.clone()));
        Arc::new(Self {
            url,
            token,
            cmd,
            shared,
            registry: tokio::sync::Mutex::new(None),
            last_registry: Mutex::new(None),
            registry_events: tokio::sync::Mutex::new(false),
        })
    }

    pub fn is_alive(&self) -> bool {
        !self.shared.closed.load(Ordering::SeqCst) && !self.cmd.is_closed()
    }

    fn touch(&self) {
        *lock(&self.shared.last_used) = Instant::now();
    }

    pub fn events(&self) -> broadcast::Receiver<StreamEvent> {
        self.touch();
        self.shared.events.subscribe()
    }

    async fn request(&self, message: Value, sub: Option<SubKind>) -> Result<Value, ConnError> {
        self.touch();
        if !self.is_alive() {
            return Err(ConnError::Closed);
        }
        let what = message
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("command")
            .to_string();
        let (reply, receiver) = oneshot::channel();
        self.cmd
            .send(Cmd::Send {
                message,
                reply,
                sub,
            })
            .map_err(|_| ConnError::Closed)?;
        match tokio::time::timeout(COMMAND_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ConnError::Closed),
            Err(_) => Err(ConnError::Timeout(what)),
        }
    }

    pub async fn send(&self, message: Value) -> Result<Value, ConnError> {
        self.request(message, None).await
    }

    async fn ensure_states(&self) -> Result<(), ConnError> {
        let started = self.shared.phase.send_if_modified(|phase| {
            if *phase == Phase::Idle {
                *phase = Phase::Pending;
                true
            } else {
                false
            }
        });
        if started {
            if let Err(err) = self
                .request(
                    json!({ "type": "subscribe_entities" }),
                    Some(SubKind::Entities),
                )
                .await
            {
                self.shared.phase.send_if_modified(|phase| {
                    let reset = *phase == Phase::Pending;
                    if reset {
                        *phase = Phase::Idle;
                    }
                    reset
                });
                return Err(err);
            }
        }
        let mut receiver = self.shared.phase.subscribe();
        let waited = tokio::time::timeout(
            STATES_INIT_TIMEOUT,
            receiver.wait_for(|phase| matches!(phase, Phase::Ready | Phase::Failed)),
        )
        .await;
        match waited {
            Ok(Ok(phase)) if *phase == Phase::Ready => Ok(()),
            Ok(_) => Err(ConnError::Closed),
            Err(_) => {
                self.shared.phase.send_if_modified(|phase| {
                    let reset = *phase == Phase::Pending;
                    if reset {
                        *phase = Phase::Idle;
                    }
                    reset
                });
                Err(ConnError::Timeout("initial entity states".into()))
            }
        }
    }

    pub async fn states(&self) -> Result<HashMap<String, RawEntityState>, ConnError> {
        self.touch();
        self.ensure_states().await?;
        if !self.is_alive() {
            return Err(ConnError::Closed);
        }
        Ok(lock(&self.shared.states).clone())
    }

    /// Subscribes to registry change events; unsupported tokens keep working without live room updates.
    pub async fn ensure_registry_events(&self) -> Result<(), ConnError> {
        let mut subscribed = self.registry_events.lock().await;
        if *subscribed {
            return Ok(());
        }
        let results = futures_util::future::join_all(
            [
                "entity_registry_updated",
                "device_registry_updated",
                "area_registry_updated",
            ]
            .map(|event_type| {
                self.request(
                    json!({ "type": "subscribe_events", "event_type": event_type }),
                    Some(SubKind::Registry),
                )
            }),
        )
        .await;
        if let Some(failure) = results.into_iter().find_map(Result::err) {
            if !failure.is_code(&UNSUPPORTED_REGISTRY_CODES) {
                return Err(failure);
            }
        }
        *subscribed = true;
        // Refresh after listeners are installed, including changes during subscription setup.
        self.invalidate_registry();
        Ok(())
    }

    pub fn invalidate_registry(&self) {
        self.shared.registry_epoch.fetch_add(1, Ordering::SeqCst);
    }

    pub async fn registry(&self) -> Result<Option<HomeAssistantRegistry>, ConnError> {
        self.touch();
        let mut cache = self.registry.lock().await;
        let epoch = self.shared.registry_epoch.load(Ordering::SeqCst);
        if let Some(cached) = cache.as_ref() {
            if cached.fetched_at.elapsed() < REGISTRY_TTL && cached.epoch == epoch {
                return Ok(cached.value.clone());
            }
        }
        match self.fetch_registry().await {
            Ok(registry) => {
                *lock(&self.last_registry) = Some(registry.clone());
                *cache = Some(RegistryCache {
                    fetched_at: Instant::now(),
                    epoch,
                    value: Some(registry.clone()),
                });
                Ok(Some(registry))
            }
            Err(error) => {
                if let Some(last) = lock(&self.last_registry).clone() {
                    tracing::warn!(
                        "Home Assistant registry refresh failed, keeping the last known rooms"
                    );
                    return Ok(Some(last));
                }
                if error.is_code(&UNSUPPORTED_REGISTRY_CODES) {
                    tracing::warn!("Home Assistant registry is not available to this token, rooms are disabled");
                    *cache = Some(RegistryCache {
                        fetched_at: Instant::now(),
                        epoch,
                        value: None,
                    });
                    return Ok(None);
                }
                tracing::warn!("Home Assistant registry could not be loaded");
                Err(error)
            }
        }
    }

    async fn fetch_registry(&self) -> Result<HomeAssistantRegistry, ConnError> {
        let (areas, floors, devices, entities) = tokio::join!(
            self.send(json!({ "type": "config/area_registry/list" })),
            async {
                match self
                    .send(json!({ "type": "config/floor_registry/list" }))
                    .await
                {
                    Err(error) if error.is_code(&["unknown_command"]) => {
                        Ok(Value::Array(Vec::new()))
                    }
                    other => other,
                }
            },
            self.send(json!({ "type": "config/device_registry/list" })),
            self.send(json!({ "type": "config/entity_registry/list_for_display" })),
        );
        parse_registry(areas?, floors?, devices?, entities?)
    }

    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        let _ = self.cmd.send(Cmd::Close);
    }
}

fn parse_registry(
    areas: Value,
    floors: Value,
    devices: Value,
    entities: Value,
) -> Result<HomeAssistantRegistry, ConnError> {
    let parse_error = |what: &str, err: serde_json::Error| ConnError::Command {
        code: "invalid_response".into(),
        message: format!("Failed to parse the {what} registry: {err}"),
    };
    Ok(HomeAssistantRegistry {
        areas: serde_json::from_value::<Vec<AreaEntry>>(areas)
            .map_err(|err| parse_error("area", err))?,
        floors: serde_json::from_value::<Vec<FloorEntry>>(floors)
            .map_err(|err| parse_error("floor", err))?,
        devices: serde_json::from_value::<Vec<DeviceEntry>>(devices)
            .map_err(|err| parse_error("device", err))?,
        entities: serde_json::from_value::<EntityDisplayRegistry>(entities)
            .map_err(|err| parse_error("entity", err))?,
    })
}

type Slot = Arc<tokio::sync::Mutex<Option<Arc<HaConnection>>>>;
type Pool = Mutex<HashMap<String, Slot>>;

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `Integration.webSocketUrl("/api/websocket")`.
pub fn websocket_url(ctx: &IntegrationContext) -> HomarrResult<Url> {
    ws::to_ws_url(crate::groups::home_misc::util::url(
        ctx,
        "/api/websocket",
        &[],
    )?)
}

/// Returns the shared connection of the integration, reconnecting when it died or its configuration changed.
/// Uses a per-integration mutex so connecting to one unreachable server never stalls other instances.
pub async fn acquire(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Arc<HaConnection>> {
    let url = websocket_url(ctx)?;
    let token = ctx.require_secret("apiKey")?.to_string();
    let slot = {
        let mut map = lock(pool());
        map.entry(ctx.id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
            .clone()
    };
    let mut guard = slot.lock().await;
    if let Some(existing) = guard.as_ref() {
        if existing.is_alive() && existing.url == url && existing.token == token {
            existing.touch();
            return Ok(existing.clone());
        }
        // The saved URL or token changed: close the stale connection before opening a new one.
        existing.close();
        *guard = None;
    }
    let socket = open_authenticated(rt, ctx, &url, &token).await?;
    let connection = HaConnection::start(socket, url, token);
    *guard = Some(connection.clone());
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(value: Value) -> CompressedStateEvent {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn initial_event_replaces_states_and_diffs_patch_them() {
        let mut states = HashMap::new();
        states.insert(
            "stale.x".to_string(),
            RawEntityState {
                entity_id: "stale.x".into(),
                state: "on".into(),
                attributes: Default::default(),
                last_changed: 0.0,
                last_updated: 0.0,
                context_id: None,
            },
        );
        let initial = apply_compressed_event(
            &mut states,
            &event(json!({"a": {
                "light.a": {"s": "on", "a": {"brightness": 100, "friendly_name": "A"}, "lc": 10.0, "c": "ctx1"},
                "sensor.b": {"s": "5", "lc": 11.0, "lu": 12.0}
            }})),
            true,
        );
        assert_eq!(states.len(), 2);
        assert_eq!(initial.changed.len(), 2);
        assert_eq!(states["light.a"].last_changed, 10_000.0);
        assert_eq!(states["light.a"].last_updated, 10_000.0);
        assert_eq!(states["light.a"].context_id.as_deref(), Some("ctx1"));
        assert_eq!(states["sensor.b"].last_updated, 12_000.0);

        let change = apply_compressed_event(
            &mut states,
            &event(json!({
                "c": {"light.a": {"+": {"s": "off", "a": {"effect": "x"}, "lu": 20.0, "c": {"id": "ctx2"}}, "-": {"a": ["brightness"]}},
                      "missing.z": {"+": {"s": "1"}}},
                "r": ["sensor.b", "unknown.q"]
            })),
            false,
        );
        assert_eq!(change.removed, vec!["sensor.b"]);
        assert_eq!(change.changed.len(), 1);
        let light = &states["light.a"];
        assert_eq!(light.state, "off");
        assert_eq!(light.last_changed, 10_000.0);
        assert_eq!(light.last_updated, 20_000.0);
        assert_eq!(light.context_id.as_deref(), Some("ctx2"));
        assert!(light.attributes.get("brightness").is_none());
        assert_eq!(light.attributes["effect"], json!("x"));
        assert_eq!(light.attributes["friendly_name"], json!("A"));
        assert!(!states.contains_key("sensor.b") && !states.contains_key("missing.z"));
    }

    #[test]
    fn command_errors_identify_unsupported_registry_codes() {
        let unsupported = ConnError::Command {
            code: "unknown_command".into(),
            message: String::new(),
        };
        assert!(unsupported.is_code(&UNSUPPORTED_REGISTRY_CODES));
        assert!(!ConnError::Closed.is_code(&UNSUPPORTED_REGISTRY_CODES));
        assert!(parse_registry(json!([]), json!([]), json!([]), json!({"entities": []})).is_ok());
        assert!(parse_registry(json!({}), json!([]), json!([]), json!({})).is_err());
    }
}
