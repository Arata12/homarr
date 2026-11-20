//! Home Assistant: smart home snapshot/catalog/history/forecast/actions/subscription over the websocket API,
//! plus the REST calendar (`homeassistant-integration.ts`).

mod actions;
mod connection;
mod history;
mod normalize;
mod types;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use homarr_common::{HomarrError, HomarrResult};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use self::actions::{action_type, resolve_action, supports_action, to_service_call};
use self::connection::{acquire, open_authenticated, websocket_url, ConnError, StreamEvent};
use self::history::{bucket_history, HistoryState};
use self::normalize::{
    create_catalog, get_entity_domain, normalize_areas, normalize_entity, EntityRegistryContext,
    RegistryLookup,
};
use self::types::{CalendarMoment, CalendarRef, HaCalendarEvent, HistoryItem, RawEntityState};
use super::util::{get_json, input_u64, q, send, to_json, unknown_operation, url};
use crate::capabilities::calendar::CalendarEvent;
use crate::capabilities::smart_home::{
    SmartHomeAction, SmartHomeActionReceipt, SmartHomeArea, SmartHomeCatalog, SmartHomeEntity,
    SmartHomeEntityPatch, SmartHomeEntitySelection, SmartHomeForecastDay, SmartHomeForecastType,
    SmartHomeHistorySeries, SmartHomeSnapshot, SMART_HOME_ENTITY_LIMIT,
};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

fn headers(ctx: &IntegrationContext) -> HomarrResult<Vec<(&'static str, String)>> {
    Ok(vec![(
        "Authorization",
        format!("Bearer {}", ctx.require_secret("apiKey")?),
    )])
}

fn conn_err(ctx: &IntegrationContext) -> impl Fn(ConnError) -> HomarrError + '_ {
    move |error| error.into_homarr(ctx)
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let mut request = rt.http.standard().get(url(ctx, "/api/config", &[])?);
    for (name, value) in headers(ctx)? {
        request = request.header(name, value);
    }
    send(ctx, request).await?;
    let socket =
        open_authenticated(rt, ctx, &websocket_url(ctx)?, ctx.require_secret("apiKey")?).await?;
    let mut socket = socket;
    let _ = futures_util::SinkExt::close(&mut socket).await;
    Ok(())
}

// ---- selection ---------------------------------------------------------------------------------

fn resolve_selection(
    states: &HashMap<String, RawEntityState>,
    lookup: &RegistryLookup,
    selection: &SmartHomeEntitySelection,
) -> Vec<String> {
    let entity_ids = selection.entity_ids.clone().unwrap_or_default();
    let explicit: Vec<String> = entity_ids
        .iter()
        .filter(|id| states.contains_key(*id))
        .cloned()
        .collect();
    let domains: Option<HashSet<&str>> = selection
        .domains
        .as_ref()
        .filter(|domains| !domains.is_empty())
        .map(|domains| domains.iter().map(String::as_str).collect());
    let area_ids: Option<HashSet<&str>> = selection
        .area_ids
        .as_ref()
        .filter(|areas| !areas.is_empty())
        .map(|areas| areas.iter().map(String::as_str).collect());
    let excluded: HashSet<&str> = selection
        .excluded_entity_ids
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let include_all = explicit.is_empty() && area_ids.is_none() && entity_ids.is_empty();

    let mut implicit: Vec<&String> = states
        .keys()
        .filter(|id| {
            let context = lookup.get(id);
            if context.hidden || excluded.contains(id.as_str()) {
                return false;
            }
            if domains
                .as_ref()
                .is_some_and(|domains| !domains.contains(get_entity_domain(id)))
            {
                return false;
            }
            if let Some(areas) = &area_ids {
                return context
                    .area_id
                    .as_deref()
                    .is_some_and(|area| areas.contains(area));
            }
            include_all
        })
        .collect();
    implicit.sort();

    let mut seen = HashSet::new();
    explicit
        .into_iter()
        .chain(implicit.into_iter().cloned())
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

fn resolve_snapshot_ids(
    states: &HashMap<String, RawEntityState>,
    lookup: &RegistryLookup,
    areas: &[SmartHomeArea],
    selection: &SmartHomeEntitySelection,
) -> HomarrResult<Vec<String>> {
    let excluded: HashSet<&str> = selection
        .excluded_entity_ids
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let domains: HashSet<&str> = selection
        .domains
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let selected_areas = selection.area_ids.clone().unwrap_or_default();
    let ambient: Vec<String> = areas
        .iter()
        .filter(|area| selected_areas.contains(&area.id))
        .flat_map(|area| {
            [
                area.temperature_entity_id.clone(),
                area.humidity_entity_id.clone(),
            ]
        })
        .flatten()
        .filter(|id| {
            states.contains_key(id)
                && !lookup.get(id).hidden
                && !excluded.contains(id.as_str())
                && (domains.is_empty() || domains.contains(get_entity_domain(id)))
        })
        .collect();
    let mut seen = HashSet::new();
    let ids: Vec<String> = resolve_selection(states, lookup, selection)
        .into_iter()
        .chain(ambient)
        .filter(|id| seen.insert(id.clone()))
        .collect();
    if ids.len() > SMART_HOME_ENTITY_LIMIT {
        return Err(HomarrError::BadRequest(format!(
            "Selected {} Home Assistant entities; the maximum is {SMART_HOME_ENTITY_LIMIT}. Narrow the selection.",
            ids.len()
        )));
    }
    Ok(ids)
}

fn used_areas(areas: &[SmartHomeArea], used: &BTreeSet<Option<String>>) -> Vec<SmartHomeArea> {
    areas
        .iter()
        .filter(|area| used.contains(&Some(area.id.clone())))
        .cloned()
        .collect()
}

/// Camera snapshots get a cache-busting parameter that changes this often.
const CAMERA_REFRESH_MS: f64 = 10_000.0;

/// `getPictureUrlAsync`: entity pictures (and camera snapshots) are served through the image proxy.
async fn picture_url(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    raw: &RawEntityState,
) -> Option<String> {
    let picture = raw
        .attributes
        .get("entity_picture")
        .and_then(Value::as_str)
        .filter(|picture| !picture.is_empty())?;
    if raw.state == "unavailable" {
        return None;
    }
    let mut source = url::Url::parse(ctx.url.trim()).ok()?.join(picture).ok()?;
    let is_camera = get_entity_domain(&raw.entity_id) == "camera";
    let mut pairs: Vec<(String, String)> = source
        .query_pairs()
        .filter(|(key, _)| key != "token" && !(is_camera && (key == "width" || key == "homarr")))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if is_camera {
        pairs.push(("width".into(), "640".into()));
        let bucket = (raw.last_updated / CAMERA_REFRESH_MS).floor() as i64;
        pairs.push(("homarr".into(), bucket.to_string()));
    }
    if pairs.is_empty() {
        source.set_query(None);
    } else {
        source.query_pairs_mut().clear().extend_pairs(pairs);
    }
    let headers = headers(ctx)
        .ok()?
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
    super::gotify::proxy_integration_image(rt, ctx, &source, headers).await
}

/// Attaches proxied `pictureUrl`s to `entities`, looking each one up in `states`.
async fn attach_pictures(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    states: &HashMap<String, RawEntityState>,
    entities: &mut [SmartHomeEntity],
) {
    if rt.image_proxy.is_none() {
        return;
    }
    let urls = futures_util::future::join_all(entities.iter().map(|entity| async move {
        match states.get(&entity.id) {
            Some(raw) => picture_url(rt, ctx, raw).await,
            None => None,
        }
    }))
    .await;
    for (entity, url) in entities.iter_mut().zip(urls) {
        if url.is_some() {
            entity.attributes.picture_url = url;
        }
    }
}

fn to_entities(raws: &[&RawEntityState], lookup: &RegistryLookup) -> Vec<SmartHomeEntity> {
    raws.iter()
        .map(|raw| normalize_entity(raw, &lookup.get(&raw.entity_id)))
        .collect()
}

// ---- reads -------------------------------------------------------------------------------------

pub async fn snapshot(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    selection: &SmartHomeEntitySelection,
) -> HomarrResult<SmartHomeSnapshot> {
    let connection = acquire(rt, ctx).await?;
    let (states, registry) =
        tokio::try_join!(connection.states(), connection.registry()).map_err(conn_err(ctx))?;
    let lookup = RegistryLookup::new(registry.as_ref());
    let areas = normalize_areas(registry.as_ref());
    let ids = resolve_snapshot_ids(&states, &lookup, &areas, selection)?;
    let raws: Vec<&RawEntityState> = ids.iter().filter_map(|id| states.get(id)).collect();
    let mut entities = to_entities(&raws, &lookup);
    attach_pictures(rt, ctx, &states, &mut entities).await;
    let mut used: BTreeSet<Option<String>> = entities
        .iter()
        .map(|entity| entity.area_id.clone())
        .collect();
    used.extend(
        selection
            .area_ids
            .iter()
            .flatten()
            .map(|id| Some(id.clone())),
    );
    Ok(SmartHomeSnapshot {
        areas: used_areas(&areas, &used),
        entities,
        updated_at: Utc::now().timestamp_millis() as f64,
    })
}

pub async fn catalog(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<SmartHomeCatalog> {
    let connection = acquire(rt, ctx).await?;
    let (states, registry) =
        tokio::try_join!(connection.states(), connection.registry()).map_err(conn_err(ctx))?;
    Ok(create_catalog(states.values(), registry.as_ref()))
}

pub async fn history_series(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    entity_ids: &[String],
    hours: u32,
    samples: u32,
) -> HomarrResult<Vec<SmartHomeHistorySeries>> {
    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }
    let end = Utc::now().timestamp_millis() as f64;
    let start = end - f64::from(hours) * 3_600_000.0;
    let iso = |millis: f64| {
        DateTime::<Utc>::from_timestamp_millis(millis as i64)
            .unwrap_or_default()
            .to_rfc3339_opts(SecondsFormat::Millis, true)
    };
    let connection = acquire(rt, ctx).await?;
    let result = connection
        .send(json!({
            "type": "history/history_during_period",
            "start_time": iso(start),
            "end_time": iso(end),
            "entity_ids": entity_ids,
            "minimal_response": true,
            "no_attributes": true,
            "significant_changes_only": false,
        }))
        .await
        .map_err(conn_err(ctx))?;
    let history: HashMap<String, Vec<HistoryItem>> = serde_json::from_value(result)
        .map_err(|err| crate::error::parse_error(ctx, "Home Assistant history", err))?;
    Ok(entity_ids
        .iter()
        .map(|entity_id| {
            let states: Vec<HistoryState> = history
                .get(entity_id)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| HistoryState {
                            state: item.s.clone(),
                            time: item.lu.or(item.lc).unwrap_or(0.0) * 1000.0,
                        })
                        .collect()
                })
                .unwrap_or_default();
            SmartHomeHistorySeries {
                entity_id: entity_id.clone(),
                points: bucket_history(&states, start, end, samples),
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct ForecastResult {
    response: HashMap<String, ForecastEntry>,
}

#[derive(Debug, Deserialize)]
struct ForecastEntry {
    forecast: Vec<ForecastItem>,
}

#[derive(Debug, Deserialize)]
struct ForecastItem {
    datetime: String,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    templow: Option<f64>,
    #[serde(default)]
    precipitation_probability: Option<f64>,
}

fn parse_forecast(
    ctx: &IntegrationContext,
    entity_id: &str,
    result: Value,
) -> HomarrResult<Vec<SmartHomeForecastDay>> {
    let parsed: ForecastResult = serde_json::from_value(result)
        .map_err(|err| crate::error::parse_error(ctx, "Home Assistant forecast", err))?;
    parsed
        .response
        .get(entity_id)
        .map(|entry| entry.forecast.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|item| {
            let time = DateTime::parse_from_rfc3339(&item.datetime)
                .map(|time| time.timestamp_millis())
                .or_else(|_| {
                    NaiveDateTime::parse_from_str(&item.datetime, "%Y-%m-%dT%H:%M:%S%.f")
                        .map(|naive| naive.and_utc().timestamp_millis())
                })
                .or_else(|_| {
                    NaiveDate::parse_from_str(&item.datetime, "%Y-%m-%d").map(|date| {
                        date.and_hms_opt(0, 0, 0)
                            .unwrap_or_default()
                            .and_utc()
                            .timestamp_millis()
                    })
                })
                .map_err(|err| {
                    crate::error::parse_error(ctx, "Home Assistant forecast time", err)
                })?;
            Ok(SmartHomeForecastDay {
                time: time as f64,
                condition: item.condition.clone().unwrap_or_else(|| "unknown".into()),
                temperature_high: item.temperature,
                temperature_low: item.templow,
                precipitation_probability: item.precipitation_probability,
            })
        })
        .collect()
}

pub async fn forecast(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    entity_id: &str,
    forecast_type: SmartHomeForecastType,
) -> HomarrResult<Vec<SmartHomeForecastDay>> {
    let connection = acquire(rt, ctx).await?;
    let kind = match forecast_type {
        SmartHomeForecastType::Daily => "daily",
        SmartHomeForecastType::Hourly => "hourly",
    };
    let result = connection
        .send(json!({
            "type": "call_service",
            "domain": "weather",
            "service": "get_forecasts",
            "service_data": { "type": kind },
            "target": { "entity_id": entity_id },
            "return_response": true,
        }))
        .await
        .map_err(conn_err(ctx))?;
    parse_forecast(ctx, entity_id, result)
}

pub async fn execute_action(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    entity_id: &str,
    action: &SmartHomeAction,
) -> HomarrResult<Option<SmartHomeActionReceipt>> {
    let not_supported = || {
        HomarrError::BadRequest(format!(
            "Action {} is not supported for {entity_id}",
            action_type(action)
        ))
    };
    let connection = acquire(rt, ctx).await?;
    let states = connection.states().await.map_err(conn_err(ctx))?;
    let current = states
        .get(entity_id)
        .ok_or_else(|| HomarrError::NotFound(format!("Entity {entity_id} was not found")))?;
    let entity = normalize_entity(current, &EntityRegistryContext::default());
    if !supports_action(&entity, action) {
        return Err(not_supported());
    }
    let resolved = resolve_action(&entity, action);
    if !supports_action(&entity, &resolved) {
        return Err(not_supported());
    }
    let service_call = to_service_call(entity_id, &resolved).ok_or_else(not_supported)?;
    let result = connection
        .send(json!({
            "type": "call_service",
            "domain": service_call.domain,
            "service": service_call.service,
            "service_data": service_call.data,
            "target": { "entity_id": entity_id },
        }))
        .await
        .map_err(conn_err(ctx))?;
    Ok(result
        .get("context")
        .and_then(|context| context.get("id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| SmartHomeActionReceipt {
            context_id: id.to_string(),
        }))
}

// ---- subscription ------------------------------------------------------------------------------

/// Tracks which entities of the selection are visible to the subscriber and what still has to be sent.
struct SubscriptionState {
    selection: SmartHomeEntitySelection,
    states: HashMap<String, RawEntityState>,
    lookup: RegistryLookup,
    areas: Vec<SmartHomeArea>,
    watched: HashSet<String>,
    present: HashSet<String>,
    pending_changed: HashMap<String, RawEntityState>,
    pending_removed: HashSet<String>,
    pending_areas: Option<Vec<SmartHomeArea>>,
}

impl SubscriptionState {
    fn reconcile_selection(&mut self, metadata_changed: bool) -> HomarrResult<()> {
        let ids = resolve_snapshot_ids(&self.states, &self.lookup, &self.areas, &self.selection)?;
        let selected: HashSet<&String> = ids.iter().collect();
        self.watched = ids
            .iter()
            .cloned()
            .chain(self.selection.entity_ids.iter().flatten().cloned())
            .collect();
        let mut membership_changed = false;
        let previous: Vec<String> = self.present.iter().cloned().collect();
        for id in previous {
            if selected.contains(&id) {
                continue;
            }
            membership_changed = true;
            self.pending_changed.remove(&id);
            self.pending_removed.insert(id);
        }
        for id in &ids {
            if let Some(raw) = self.states.get(id) {
                if metadata_changed || !self.present.contains(id) {
                    self.pending_changed.insert(id.clone(), raw.clone());
                }
            }
            if !self.present.contains(id) {
                membership_changed = true;
            }
            self.pending_removed.remove(id);
        }
        self.present = ids.iter().cloned().collect();
        if metadata_changed || membership_changed {
            let mut used: BTreeSet<Option<String>> =
                ids.iter().map(|id| self.lookup.get(id).area_id).collect();
            used.extend(
                self.selection
                    .area_ids
                    .iter()
                    .flatten()
                    .map(|id| Some(id.clone())),
            );
            self.pending_areas = Some(used_areas(&self.areas, &used));
        }
        if metadata_changed {
            for id in self.selection.entity_ids.iter().flatten() {
                if !self.states.contains_key(id) {
                    self.pending_removed.insert(id.clone());
                }
            }
        }
        Ok(())
    }

    fn on_change(&mut self, change: &connection::StateChange) -> HomarrResult<()> {
        for raw in &change.changed {
            self.states.insert(raw.entity_id.clone(), raw.clone());
        }
        for id in &change.removed {
            self.states.remove(id);
        }
        self.reconcile_selection(false)?;
        for raw in &change.changed {
            if !self.watched.contains(&raw.entity_id) {
                continue;
            }
            self.pending_changed
                .insert(raw.entity_id.clone(), raw.clone());
            self.pending_removed.remove(&raw.entity_id);
        }
        for id in &change.removed {
            if !self.watched.contains(id) {
                continue;
            }
            self.pending_changed.remove(id);
            self.pending_removed.insert(id.clone());
        }
        Ok(())
    }

    fn has_pending(&self) -> bool {
        !self.pending_changed.is_empty()
            || !self.pending_removed.is_empty()
            || self.pending_areas.is_some()
    }

    fn take_patch(&mut self) -> Option<SmartHomeEntityPatch> {
        if !self.has_pending() {
            return None;
        }
        let changed_raws: Vec<RawEntityState> = std::mem::take(&mut self.pending_changed)
            .into_values()
            .collect();
        let removed: Vec<String> = std::mem::take(&mut self.pending_removed)
            .into_iter()
            .collect();
        let areas = self.pending_areas.take();
        let refs: Vec<&RawEntityState> = changed_raws
            .iter()
            .filter(|raw| !removed.contains(&raw.entity_id))
            .collect();
        Some(SmartHomeEntityPatch {
            changed: to_entities(&refs, &self.lookup),
            removed,
            areas,
        })
    }
}

pub async fn subscribe(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    selection: &SmartHomeEntitySelection,
    patches: mpsc::UnboundedSender<SmartHomeEntityPatch>,
    mut cancel: watch::Receiver<bool>,
) -> HomarrResult<()> {
    if *cancel.borrow() {
        return Ok(());
    }
    let connection = acquire(rt, ctx).await?;
    let mut events = connection.events();
    // Registry events are optional: tokens without access keep working without live room updates.
    if let Err(error) = connection.ensure_registry_events().await {
        tracing::debug!(reason = ?error, "Home Assistant registry events are unavailable");
    }
    let states = connection.states().await.map_err(conn_err(ctx))?;
    let registry = connection.registry().await.map_err(conn_err(ctx))?;
    let mut state = SubscriptionState {
        selection: selection.clone(),
        states,
        lookup: RegistryLookup::new(registry.as_ref()),
        areas: normalize_areas(registry.as_ref()),
        watched: HashSet::new(),
        present: HashSet::new(),
        pending_changed: HashMap::new(),
        pending_removed: HashSet::new(),
        pending_areas: None,
    };
    state.reconcile_selection(true)?;
    flush(rt, ctx, &mut state, &patches).await;

    let mut registry_poll = tokio::time::interval(Duration::from_secs(60));
    registry_poll.tick().await;
    let mut registry_dirty_at: Option<tokio::time::Instant> = None;

    loop {
        let debounce = async {
            match registry_dirty_at {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    return Ok(());
                }
            }
            event = events.recv() => match event {
                Ok(StreamEvent::Change(change)) => {
                    state.on_change(&change)?;
                    flush(rt, ctx, &mut state, &patches).await;
                }
                Ok(StreamEvent::RegistryChanged) => {
                    registry_dirty_at.get_or_insert_with(|| tokio::time::Instant::now() + Duration::from_millis(100));
                }
                Ok(StreamEvent::Closed) | Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    return Err(ConnError::Closed.into_homarr(ctx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // Missed events: resynchronize from the connection cache.
                    state.states = connection.states().await.map_err(conn_err(ctx))?;
                    state.reconcile_selection(true)?;
                    flush(rt, ctx, &mut state, &patches).await;
                }
            },
            _ = registry_poll.tick() => {
                connection.invalidate_registry();
                registry_dirty_at.get_or_insert_with(tokio::time::Instant::now);
            }
            () = debounce => {
                registry_dirty_at = None;
                let registry = connection.registry().await.map_err(conn_err(ctx))?;
                state.lookup = RegistryLookup::new(registry.as_ref());
                state.areas = normalize_areas(registry.as_ref());
                state.reconcile_selection(true)?;
                flush(rt, ctx, &mut state, &patches).await;
            }
        }
    }
}

async fn flush(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    state: &mut SubscriptionState,
    patches: &mpsc::UnboundedSender<SmartHomeEntityPatch>,
) {
    if let Some(mut patch) = state.take_patch() {
        attach_pictures(rt, ctx, &state.states, &mut patch.changed).await;
        // A closed receiver means the subscriber went away; the cancel signal ends the loop.
        let _ = patches.send(patch);
    }
}

// ---- calendar (REST) ---------------------------------------------------------------------------

fn moment_to_utc(moment: &CalendarMoment) -> HomarrResult<DateTime<Utc>> {
    match moment {
        CalendarMoment::Date { date } => {
            let naive = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|err| {
                    HomarrError::Upstream(format!("Invalid calendar date '{date}': {err}"))
                })?
                .and_hms_opt(0, 0, 0)
                .unwrap_or_default();
            // `new Date("YYYY-MM-DDT00:00:00")` is local midnight.
            Ok(chrono::Local
                .from_local_datetime(&naive)
                .earliest()
                .map_or_else(|| naive.and_utc(), |local| local.with_timezone(&Utc)))
        }
        CalendarMoment::DateTime { date_time } => DateTime::parse_from_rfc3339(date_time)
            .map(|time| time.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDateTime::parse_from_str(date_time, "%Y-%m-%dT%H:%M:%S%.f")
                    .map(|naive| naive.and_utc())
            })
            .map_err(|err| {
                HomarrError::Upstream(format!("Invalid calendar time '{date_time}': {err}"))
            }),
    }
}

fn map_calendar_event(event: &HaCalendarEvent) -> HomarrResult<CalendarEvent> {
    Ok(CalendarEvent {
        title: event.summary.clone(),
        sub_title: None,
        description: event.description.clone(),
        start_date: moment_to_utc(&event.start)?,
        end_date: Some(moment_to_utc(&event.end)?),
        is_all_day: None,
        image: None,
        location: event.location.clone(),
        metadata: None,
        indicator_color: "#18bcf2".into(),
        links: Vec::new(),
    })
}

pub async fn calendar_events(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> HomarrResult<Vec<CalendarEvent>> {
    let auth = headers(ctx)?;
    let calendars: Vec<CalendarRef> = get_json(
        ctx,
        &rt.http,
        "Home Assistant calendars",
        url(ctx, "/api/calendars", &[])?,
        &auth,
    )
    .await?;
    let iso = |time: DateTime<Utc>| time.to_rfc3339_opts(SecondsFormat::Millis, true);
    let per_calendar = futures_util::future::try_join_all(calendars.iter().map(|calendar| {
        let auth = &auth;
        async move {
            let query = [("start", q(iso(start))), ("end", q(iso(end)))];
            get_json::<Vec<HaCalendarEvent>>(
                ctx,
                &rt.http,
                "Home Assistant calendar events",
                url(
                    ctx,
                    &format!("/api/calendars/{}", calendar.entity_id),
                    &query,
                )?,
                auth,
            )
            .await
        }
    }))
    .await?;
    per_calendar
        .iter()
        .flatten()
        .map(map_calendar_event)
        .collect()
}

// ---- entity stats ------------------------------------------------------------------------------

pub async fn entity_stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Value> {
    let states = acquire(rt, ctx)
        .await?
        .states()
        .await
        .map_err(conn_err(ctx))?;
    let count = |predicate: &dyn Fn(&RawEntityState) -> bool| {
        states.values().filter(|state| predicate(state)).count()
    };
    Ok(json!({
        "entities": states.len(),
        "unavailable": count(&|s| s.state == "unavailable"),
        "lightsOn": count(&|s| s.entity_id.starts_with("light.") && s.state == "on"),
        "peopleHome": count(&|s| s.entity_id.starts_with("person.") && s.state == "home"),
    }))
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    _input: &Value,
) -> HomarrResult<Value> {
    let _ = input_u64;
    match operation {
        "getEntityStats" => entity_stats(rt, ctx).await,
        "getCatalog" => to_json(&catalog(rt, ctx).await?),
        other => Err(unknown_operation(ctx, other)),
    }
}

// ---- legacy adapters ---------------------------------------------------------------------------

/// First 50 entities by id in the shape of the previous implementation.
pub async fn legacy_entities(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<crate::types::SmartHomeEntity>> {
    let states = acquire(rt, ctx)
        .await?
        .states()
        .await
        .map_err(conn_err(ctx))?;
    let mut raws: Vec<&RawEntityState> = states.values().collect();
    raws.sort_by(|left, right| left.entity_id.cmp(&right.entity_id));
    Ok(raws
        .into_iter()
        .take(50)
        .map(|raw| {
            let entity = normalize_entity(raw, &EntityRegistryContext::default());
            crate::types::SmartHomeEntity {
                friendly_name: entity.name,
                entity_id: entity.id,
                state: entity.state,
                unit_of_measurement: entity.unit,
                domain: entity.domain,
            }
        })
        .collect())
}

/// Maps a Home Assistant service name (as sent by the old API) to a smart home action.
pub fn action_for_service(service: &str) -> Option<SmartHomeAction> {
    Some(match service {
        "toggle" => SmartHomeAction::Toggle,
        "turn_on" => SmartHomeAction::TurnOn,
        "turn_off" => SmartHomeAction::TurnOff,
        "open" | "open_cover" | "open_valve" => SmartHomeAction::Open,
        "close" | "close_cover" | "close_valve" => SmartHomeAction::Close,
        "stop" | "stop_cover" | "stop_valve" => SmartHomeAction::Stop,
        "lock" => SmartHomeAction::Lock { code: None },
        "unlock" => SmartHomeAction::Unlock { code: None },
        "press" | "trigger" => SmartHomeAction::Trigger,
        "media_play_pause" => SmartHomeAction::MediaPlayPause,
        "media_play" => SmartHomeAction::MediaPlay,
        "media_pause" => SmartHomeAction::MediaPause,
        "media_next_track" => SmartHomeAction::MediaNext,
        "media_previous_track" => SmartHomeAction::MediaPrevious,
        "increment" => SmartHomeAction::Increment,
        "decrement" => SmartHomeAction::Decrement,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;

    use super::*;
    use crate::capabilities::smart_home::SmartHomeEntityAttributes;
    use crate::groups::home_misc::homeassistant::types::{
        AreaEntry, EntityDisplayRegistry, EntityRegistryEntry, HomeAssistantRegistry,
    };

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "ha".into(),
            name: "Home Assistant".into(),
            url: "http://ha.local:8123".into(),
            external_url: None,
            kind: IntegrationKind::HomeAssistant,
            secrets: HashMap::new(),
        }
    }

    fn raw(id: &str, state: &str) -> RawEntityState {
        RawEntityState {
            entity_id: id.into(),
            state: state.into(),
            attributes: Default::default(),
            last_changed: 0.0,
            last_updated: 0.0,
            context_id: None,
        }
    }

    fn registry() -> HomeAssistantRegistry {
        let entry = |id: &str, area: Option<&str>, hidden: bool| EntityRegistryEntry {
            ei: id.into(),
            ai: area.map(str::to_string),
            di: None,
            en: None,
            ic: None,
            ec: None,
            hb: Some(hidden),
        };
        HomeAssistantRegistry {
            areas: vec![
                AreaEntry {
                    area_id: "kitchen".into(),
                    name: "Kitchen".into(),
                    floor_id: None,
                    icon: None,
                    temperature_entity_id: Some("sensor.kitchen_temp".into()),
                    humidity_entity_id: None,
                },
                AreaEntry {
                    area_id: "office".into(),
                    name: "Office".into(),
                    floor_id: None,
                    icon: None,
                    temperature_entity_id: None,
                    humidity_entity_id: None,
                },
            ],
            floors: vec![],
            devices: vec![],
            entities: EntityDisplayRegistry {
                entities: vec![
                    entry("light.kitchen", Some("kitchen"), false),
                    entry("sensor.kitchen_temp", Some("kitchen"), false),
                    entry("switch.office", Some("office"), false),
                    entry("light.hidden", Some("kitchen"), true),
                ],
            },
        }
    }

    fn states() -> HashMap<String, RawEntityState> {
        [
            "light.kitchen",
            "sensor.kitchen_temp",
            "switch.office",
            "light.hidden",
            "sensor.other",
        ]
        .into_iter()
        .map(|id| (id.to_string(), raw(id, "on")))
        .collect()
    }

    #[test]
    fn selects_by_area_domain_and_hidden_flag() {
        let registry = registry();
        let lookup = RegistryLookup::new(Some(&registry));
        let areas = normalize_areas(Some(&registry));
        let states = states();

        let all = resolve_snapshot_ids(
            &states,
            &lookup,
            &areas,
            &SmartHomeEntitySelection::default(),
        )
        .unwrap();
        assert_eq!(
            all,
            vec![
                "light.kitchen",
                "sensor.kitchen_temp",
                "sensor.other",
                "switch.office"
            ]
        );

        let kitchen = resolve_snapshot_ids(
            &states,
            &lookup,
            &areas,
            &SmartHomeEntitySelection {
                area_ids: Some(vec!["kitchen".into()]),
                domains: Some(vec!["light".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        // The area's ambient temperature sensor is added even though the domain filter excludes sensors.
        assert_eq!(kitchen, vec!["light.kitchen"]);

        let explicit = resolve_snapshot_ids(
            &states,
            &lookup,
            &areas,
            &SmartHomeEntitySelection {
                entity_ids: Some(vec!["light.hidden".into(), "missing.x".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(explicit, vec!["light.hidden"]);

        let area_only = resolve_snapshot_ids(
            &states,
            &lookup,
            &areas,
            &SmartHomeEntitySelection {
                area_ids: Some(vec!["kitchen".into()]),
                excluded_entity_ids: Some(vec!["light.kitchen".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(area_only, vec!["sensor.kitchen_temp"]);
    }

    #[test]
    fn selection_limit_is_enforced() {
        let registry = HomeAssistantRegistry::default();
        let lookup = RegistryLookup::new(Some(&registry));
        let states: HashMap<String, RawEntityState> = (0..=SMART_HOME_ENTITY_LIMIT)
            .map(|index| {
                (
                    format!("sensor.s{index}"),
                    raw(&format!("sensor.s{index}"), "1"),
                )
            })
            .collect();
        let error =
            resolve_snapshot_ids(&states, &lookup, &[], &SmartHomeEntitySelection::default())
                .unwrap_err();
        assert!(error.to_string().contains("maximum is 400"));
    }

    #[test]
    fn subscription_emits_initial_snapshot_then_membership_patches() {
        let registry = registry();
        let mut state = SubscriptionState {
            selection: SmartHomeEntitySelection {
                area_ids: Some(vec!["office".into()]),
                ..Default::default()
            },
            states: states(),
            lookup: RegistryLookup::new(Some(&registry)),
            areas: normalize_areas(Some(&registry)),
            watched: HashSet::new(),
            present: HashSet::new(),
            pending_changed: HashMap::new(),
            pending_removed: HashSet::new(),
            pending_areas: None,
        };
        state.reconcile_selection(true).unwrap();
        let initial = state.take_patch().unwrap();
        assert_eq!(initial.changed.len(), 1);
        assert_eq!(initial.changed[0].id, "switch.office");
        assert_eq!(initial.areas.unwrap()[0].id, "office");
        assert!(state.take_patch().is_none());

        // Unwatched entity changes are ignored; watched ones are forwarded.
        state
            .on_change(&connection::StateChange {
                changed: vec![raw("light.kitchen", "off"), raw("switch.office", "off")],
                removed: vec![],
            })
            .unwrap();
        let patch = state.take_patch().unwrap();
        assert_eq!(patch.changed.len(), 1);
        assert_eq!(patch.changed[0].state, "off");
        assert!(patch.areas.is_none());

        state
            .on_change(&connection::StateChange {
                changed: vec![],
                removed: vec!["switch.office".into()],
            })
            .unwrap();
        let patch = state.take_patch().unwrap();
        assert_eq!(patch.removed, vec!["switch.office"]);
        assert!(patch.areas.is_some());
    }

    #[test]
    fn parses_forecast_response() {
        let result = json!({"response": {"weather.home": {"forecast": [
            {"datetime": "2025-01-02T00:00:00+00:00", "condition": "sunny", "temperature": 7.5, "templow": 1.0, "precipitation_probability": 10},
            {"datetime": "2025-01-03", "temperature": null}
        ]}}});
        let days = parse_forecast(&ctx(), "weather.home", result.clone()).unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].time, 1_735_776_000_000.0);
        assert_eq!(days[0].temperature_high, Some(7.5));
        assert_eq!(days[1].condition, "unknown");
        assert!(parse_forecast(&ctx(), "weather.other", result)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn maps_calendar_events() {
        let event: HaCalendarEvent = serde_json::from_value(json!({
            "start": {"dateTime": "2025-01-02T10:00:00+01:00"},
            "end": {"dateTime": "2025-01-02T11:00:00+01:00"},
            "summary": "Dentist", "description": null, "location": "Main St"
        }))
        .unwrap();
        let mapped = map_calendar_event(&event).unwrap();
        assert_eq!(
            mapped.start_date,
            Utc.with_ymd_and_hms(2025, 1, 2, 9, 0, 0).unwrap()
        );
        assert_eq!(mapped.location.as_deref(), Some("Main St"));
        assert_eq!(mapped.indicator_color, "#18bcf2");
    }

    #[test]
    fn maps_legacy_services_to_actions() {
        assert_eq!(action_for_service("toggle"), Some(SmartHomeAction::Toggle));
        assert_eq!(
            action_for_service("open_cover"),
            Some(SmartHomeAction::Open)
        );
        assert_eq!(action_for_service("nonsense"), None);
        let _ = SmartHomeEntityAttributes::default();
    }
}
