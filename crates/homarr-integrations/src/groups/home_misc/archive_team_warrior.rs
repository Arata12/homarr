//! ArchiveTeam Warrior: status via the Seesaw SockJS websocket (`archive-team-warrior-integration.ts`).

use std::time::Duration;

use futures_util::StreamExt;
use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use super::util::{basic_auth, send, to_json, unknown_operation, url};
use super::ws;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const SNAPSHOT_TIMEOUT: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatus {
    Running,
    Completed,
    Failed,
    Canceled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WarriorItem {
    pub id: String,
    pub name: String,
    pub status: ItemStatus,
    pub project: Option<String>,
    pub start_time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Bandwidth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiving: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sending: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WarriorProjectInfo {
    pub id: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct Counts {
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub canceled: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveTeamWarriorStatus {
    pub status: String,
    pub runner_status: Option<String>,
    pub project: Option<WarriorProjectInfo>,
    pub selected_project: Option<String>,
    pub broadcast_message: Option<String>,
    pub bandwidth: Option<Bandwidth>,
    pub items: Vec<WarriorItem>,
    pub counts: Counts,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    #[serde(default)]
    project_id: Option<f64>,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawItem {
    id: String,
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    start_time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct StatusMessage {
    status: String,
}

#[derive(Debug, Deserialize)]
struct BroadcastMessage {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectSelectedMessage {
    project: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectRefreshMessage {
    #[serde(default)]
    project: Option<RawProject>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    items: Option<Vec<RawItem>>,
}

#[derive(Debug, Deserialize)]
struct StartItemMessage {
    item: RawItem,
}

#[derive(Debug, Deserialize)]
struct ItemStatusMessage {
    item_id: String,
}

#[derive(Debug, Deserialize)]
struct SeesawFrame {
    event_name: String,
    #[serde(default)]
    message: Value,
}

/// Accumulated state while the snapshot window is open.
pub struct Snapshot {
    status: String,
    runner_status: Option<String>,
    selected_project: Option<String>,
    broadcast_message: Option<String>,
    project: Option<WarriorProjectInfo>,
    bandwidth: Option<Bandwidth>,
    /// Insertion ordered (JS `Map` semantics: updating keeps the original position).
    items: Vec<WarriorItem>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            status: "Unknown".into(),
            runner_status: None,
            selected_project: None,
            broadcast_message: None,
            project: None,
            bandwidth: None,
            items: Vec::new(),
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

fn normalize_display_status(status: Option<&str>) -> String {
    match status {
        None | Some("") => "Unknown".into(),
        Some("RUNNING_PROJECT") => "Running".into(),
        Some("STOPPED") => "Stopped".into(),
        Some("IDLE") => "Idle".into(),
        Some(other) => {
            // `replaceAll("_", " ").toLowerCase().replace(/\b\w/g, upper)`
            let lowered = other.replace('_', " ").to_lowercase();
            let mut out = String::with_capacity(lowered.len());
            let mut previous_word = false;
            for character in lowered.chars() {
                let is_word = character.is_alphanumeric() || character == '_';
                if is_word && !previous_word {
                    out.extend(character.to_uppercase());
                } else {
                    out.push(character);
                }
                previous_word = is_word;
            }
            out
        }
    }
}

fn format_item_name(name: &str) -> String {
    let trimmed = name.trim_start();
    let cleaned = if trimmed.len() >= 5
        && trimmed[..4].eq_ignore_ascii_case("item")
        && trimmed[4..].starts_with(char::is_whitespace)
    {
        trimmed[4..].trim_start()
    } else {
        name
    };
    let parts: Vec<&str> = cleaned
        .split('\u{0}')
        .filter(|part| !part.is_empty())
        .collect();
    let Some(first) = parts.first() else {
        return "Item".into();
    };
    let label = if parts.len() == 1 {
        (*first).to_string()
    } else {
        format!("{first} + {} more", parts.len() - 1)
    };
    if label.chars().count() <= 80 {
        label
    } else {
        format!("{}...", label.chars().take(77).collect::<String>())
    }
}

fn normalize_item_status(status: Option<&str>) -> ItemStatus {
    match status.map(str::to_lowercase).as_deref() {
        None | Some("" | "running" | "running_project") => ItemStatus::Running,
        Some("completed") => ItemStatus::Completed,
        Some("failed" | "fail") => ItemStatus::Failed,
        Some("canceled" | "cancel") => ItemStatus::Canceled,
        _ => ItemStatus::Unknown,
    }
}

fn map_item(item: RawItem) -> WarriorItem {
    WarriorItem {
        id: item.id,
        name: format_item_name(&item.name),
        status: normalize_item_status(item.status.as_deref()),
        project: item.project,
        start_time: item.start_time,
    }
}

impl Snapshot {
    fn set_item(&mut self, item: WarriorItem) {
        match self
            .items
            .iter_mut()
            .find(|existing| existing.id == item.id)
        {
            Some(existing) => *existing = item,
            None => self.items.push(item),
        }
    }

    fn update_item_status(&mut self, id: &str, status: ItemStatus) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.status = status;
        }
    }

    pub fn apply_frame(&mut self, event_name: &str, message: &Value) {
        match event_name {
            "warrior.status" => {
                if let Some(message) = parse::<StatusMessage>(message) {
                    self.status = normalize_display_status(Some(&message.status));
                }
            }
            "warrior.project_selected" => {
                if let Some(message) = parse::<ProjectSelectedMessage>(message) {
                    self.selected_project = message.project;
                }
            }
            "warrior.broadcast_message" => {
                if let Some(message) = parse::<BroadcastMessage>(message) {
                    self.broadcast_message = message.message;
                }
            }
            "runner.status" => {
                if let Some(message) = parse::<StatusMessage>(message) {
                    self.runner_status = Some(normalize_display_status(Some(&message.status)));
                }
            }
            "project.refresh" => {
                // The message is nullable; null parses as an empty refresh.
                let refresh = if message.is_null() {
                    Some(ProjectRefreshMessage {
                        project: None,
                        status: None,
                        items: None,
                    })
                } else {
                    parse::<ProjectRefreshMessage>(message)
                };
                if let Some(refresh) = refresh {
                    self.project = refresh.project.map(|project| WarriorProjectInfo {
                        id: project.project_id.map(|id| format!("{id}")),
                        title: project.title.unwrap_or_else(|| "Unknown Project".into()),
                    });
                    if let Some(status) = refresh.status.filter(|status| !status.is_empty()) {
                        self.runner_status = Some(normalize_display_status(Some(&status)));
                    }
                    for item in refresh.items.unwrap_or_default() {
                        self.set_item(map_item(item));
                    }
                }
            }
            "pipeline.start_item" => {
                if let Some(message) = parse::<StartItemMessage>(message) {
                    self.set_item(map_item(message.item));
                }
            }
            "item.complete" | "item.fail" | "item.cancel" => {
                if let Some(message) = parse::<ItemStatusMessage>(message) {
                    let status = match event_name {
                        "item.complete" => ItemStatus::Completed,
                        "item.fail" => ItemStatus::Failed,
                        _ => ItemStatus::Canceled,
                    };
                    self.update_item_status(&message.item_id, status);
                }
            }
            "bandwidth" => {
                if let Some(message) = parse::<Bandwidth>(message) {
                    self.bandwidth = Some(message);
                }
            }
            _ => {}
        }
    }

    pub fn into_status(self, updated_at: String) -> ArchiveTeamWarriorStatus {
        let count = |status| {
            self.items
                .iter()
                .filter(|item| item.status == status)
                .count()
        };
        let counts = Counts {
            running: count(ItemStatus::Running),
            completed: count(ItemStatus::Completed),
            failed: count(ItemStatus::Failed),
            canceled: count(ItemStatus::Canceled),
        };
        ArchiveTeamWarriorStatus {
            status: self.status,
            runner_status: self.runner_status,
            project: self.project,
            selected_project: self.selected_project,
            broadcast_message: self.broadcast_message,
            bandwidth: self.bandwidth,
            items: self.items,
            counts,
            updated_at,
        }
    }
}

/// Parses one SockJS message (`o`, `h`, or `a["<frame json>", ...]`) into Seesaw frames.
fn parse_sockjs_message(raw: &str) -> Vec<(String, Value)> {
    let Some(payload) = raw.strip_prefix('a') else {
        return Vec::new();
    };
    let Ok(frames) = serde_json::from_str::<Vec<String>>(payload) else {
        return Vec::new();
    };
    frames
        .iter()
        .filter_map(|frame| serde_json::from_str::<SeesawFrame>(frame).ok())
        .map(|frame| (frame.event_name, frame.message))
        .collect()
}

fn auth_headers(ctx: &IntegrationContext) -> Vec<(&'static str, String)> {
    match (ctx.secret("username"), ctx.secret("password")) {
        (Some(username), Some(password)) => vec![("Authorization", basic_auth(username, password))],
        _ => Vec::new(),
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let mut request = rt.http.standard().get(url(ctx, "/index.html", &[])?);
    for (name, value) in auth_headers(ctx) {
        request = request.header(name, value);
    }
    send(ctx, request).await?;
    Ok(())
}

pub async fn status(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<ArchiveTeamWarriorStatus> {
    let headers = auth_headers(ctx);
    // The TS version requests index.html first (response ignored); it warms up basic auth based proxies.
    let mut request = rt.http.standard().get(url(ctx, "/index.html", &[])?);
    for (name, value) in &headers {
        request = request.header(*name, value);
    }
    let _ = super::util::send_raw(ctx, request).await?;

    let mut ws_url = ws::to_ws_url(url(ctx, "/", &[])?)?;
    ws_url.set_path(&format!(
        "/{}/{}/websocket",
        chrono::Utc::now().timestamp_millis(),
        uuid_like()
    ));
    let mut socket = ws::connect(rt, ctx, &ws_url, &headers).await?;

    let mut snapshot = Snapshot::default();
    let deadline = Instant::now() + SNAPSHOT_TIMEOUT;
    loop {
        let Ok(next) = tokio::time::timeout_at(deadline, socket.next()).await else {
            break;
        };
        match next {
            Some(Ok(Message::Text(text))) => {
                for (event, message) in parse_sockjs_message(text.as_str()) {
                    snapshot.apply_frame(&event, &message);
                }
            }
            Some(Ok(Message::Binary(bytes))) => {
                for (event, message) in parse_sockjs_message(&String::from_utf8_lossy(&bytes)) {
                    snapshot.apply_frame(&event, &message);
                }
            }
            Some(Ok(_)) => {}
            Some(Err(err)) => {
                return Err(homarr_common::HomarrError::Upstream(format!(
                    "Failed to connect to ArchiveTeam Warrior websocket: {err}"
                )))
            }
            None => break,
        }
    }
    let _ = socket.close(None).await;
    Ok(snapshot
        .into_status(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
}

fn uuid_like() -> String {
    // SockJS only needs a unique session id; time based entropy is sufficient.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    _input: &Value,
) -> HomarrResult<Value> {
    match operation {
        "getStatus" => to_json(&status(rt, ctx).await?),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn frame(event: &str, message: Value) -> String {
        format!(
            "a{}",
            serde_json::to_string(&vec![
                json!({"event_name": event, "message": message}).to_string()
            ])
            .unwrap()
        )
    }

    fn apply(snapshot: &mut Snapshot, raw: &str) {
        for (event, message) in parse_sockjs_message(raw) {
            snapshot.apply_frame(&event, &message);
        }
    }

    #[test]
    fn ignores_control_frames() {
        assert!(parse_sockjs_message("o").is_empty());
        assert!(parse_sockjs_message("h").is_empty());
        assert!(parse_sockjs_message("a not json").is_empty());
    }

    #[test]
    fn builds_status_from_frames() {
        let mut snapshot = Snapshot::default();
        apply(
            &mut snapshot,
            &frame("warrior.status", json!({"status": "RUNNING_PROJECT"})),
        );
        apply(
            &mut snapshot,
            &frame("runner.status", json!({"status": "SOME_OTHER_state"})),
        );
        apply(
            &mut snapshot,
            &frame(
                "project.refresh",
                json!({"project": {"project_id": 42, "title": "Demo"}, "items": [
                    {"id": "1", "name": "Item abc\u{0}def\u{0}ghi", "status": "running", "project": "demo", "start_time": 5}
                ]}),
            ),
        );
        apply(
            &mut snapshot,
            &frame(
                "pipeline.start_item",
                json!({"item": {"id": "2", "name": "x"}}),
            ),
        );
        apply(&mut snapshot, &frame("item.fail", json!({"item_id": "2"})));
        apply(
            &mut snapshot,
            &frame("item.complete", json!({"item_id": "unknown"})),
        );
        apply(
            &mut snapshot,
            &frame("bandwidth", json!({"received": 1, "sending": 2.5})),
        );
        let status = snapshot.into_status("now".into());
        assert_eq!(status.status, "Running");
        assert_eq!(status.runner_status.as_deref(), Some("Some Other State"));
        assert_eq!(status.project.unwrap().id.as_deref(), Some("42"));
        assert_eq!(status.items[0].name, "abc + 2 more");
        assert_eq!(status.items[1].status, ItemStatus::Failed);
        assert_eq!(
            status.counts,
            Counts {
                running: 1,
                completed: 0,
                failed: 1,
                canceled: 0
            }
        );
        assert_eq!(status.bandwidth.unwrap().sending, Some(2.5));
    }

    #[test]
    fn formats_item_names() {
        assert_eq!(format_item_name("Item "), "Item");
        assert_eq!(
            format_item_name(&format!("Item {}", "x".repeat(100)))
                .chars()
                .count(),
            80
        );
        assert_eq!(
            normalize_item_status(Some("RUNNING_PROJECT")),
            ItemStatus::Running
        );
        assert_eq!(normalize_item_status(Some("weird")), ItemStatus::Unknown);
        assert_eq!(normalize_display_status(Some("FOO_BAR")), "Foo Bar");
        assert_eq!(normalize_display_status(None), "Unknown");
    }
}
