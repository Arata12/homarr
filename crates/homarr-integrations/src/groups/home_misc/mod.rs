//! Home & misc group: smart home (Home Assistant), notifications (Ntfy, Gotify, Nextcloud, Wazuh), calendars
//! (iCal, Nextcloud, Home Assistant), search (Thingport) and assorted service integrations whose data is
//! exposed through `IntegrationGroup::integration_data` (Anchor, Umami, Paperless-ngx, Speedtest Tracker,
//! ArchiveTeam Warrior, Wazuh, Thingport, Home Assistant entity stats).

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::IntegrationKind;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use super::{Handled, IntegrationGroup};
use crate::capabilities::calendar::CalendarEvent;
use crate::capabilities::notifications::Notification;
use crate::capabilities::search::SearchResult;
use crate::capabilities::smart_home::{
    SmartHomeAction, SmartHomeActionReceipt, SmartHomeCatalog, SmartHomeEntityPatch,
    SmartHomeEntitySelection, SmartHomeForecastDay, SmartHomeForecastType, SmartHomeHistorySeries,
    SmartHomeSnapshot,
};
use crate::capabilities::stats::{StatsMetric, StatsUnit, StatsValue, StatsValues};
use crate::runtime::IntegrationRuntime;
use crate::types::{
    CalendarEvent as LegacyCalendarEvent, IntegrationContext, NotificationItem,
    SmartHomeEntity as LegacySmartHomeEntity,
};

mod anchor;
mod archive_team_warrior;
mod gotify;
mod homeassistant;
mod ical;
mod ics;
mod nextcloud;
mod ntfy;
mod paperless;
mod speedtest;
mod thingport;
mod umami;
mod util;
mod wazuh;
mod ws;

pub struct HomeMiscGroup;

const KINDS: &[IntegrationKind] = &[
    IntegrationKind::HomeAssistant,
    IntegrationKind::Thingport,
    IntegrationKind::Ntfy,
    IntegrationKind::Gotify,
    IntegrationKind::Nextcloud,
    IntegrationKind::Ical,
    IntegrationKind::PaperlessNgx,
    IntegrationKind::Anchor,
    IntegrationKind::Umami,
    IntegrationKind::ArchiveTeamWarrior,
    IntegrationKind::Wazuh,
    IntegrationKind::SpeedtestTracker,
];

fn is_home_assistant(ctx: &IntegrationContext) -> bool {
    ctx.kind == IntegrationKind::HomeAssistant
}

/// Window of the old, flat calendar API: explicit RFC 3339 bounds or a window around today.
fn legacy_window(
    start: Option<&str>,
    end: Option<&str>,
) -> HomarrResult<(DateTime<Utc>, DateTime<Utc>)> {
    let parse = |value: &str| {
        DateTime::parse_from_rfc3339(value)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|err| HomarrError::BadRequest(format!("Invalid date '{value}': {err}")))
    };
    let (default_start, default_end) = ical::default_window(Utc::now());
    Ok((
        start.map_or(Ok(default_start), parse)?,
        end.map_or(Ok(default_end), parse)?,
    ))
}

fn legacy_calendar_event(
    ctx: &IntegrationContext,
    index: usize,
    event: CalendarEvent,
) -> LegacyCalendarEvent {
    LegacyCalendarEvent {
        id: format!("{}-{}-{index}", ctx.id, event.start_date.timestamp_millis()),
        title: event.title,
        sub_title: event.sub_title,
        description: event.description,
        start_date: event.start_date.to_rfc3339(),
        end_date: event.end_date.map(|end| end.to_rfc3339()),
        image_url: event.image.map(|image| image.src),
        link: event.links.into_iter().next().map(|link| link.href),
        indicator_color: event.indicator_color,
    }
}

fn legacy_notification(notification: Notification) -> NotificationItem {
    NotificationItem {
        id: notification.id,
        title: notification.title,
        body: notification.body,
        priority: "0".into(),
        created_at: notification.time.to_rfc3339(),
    }
}

#[async_trait]
impl IntegrationGroup for HomeMiscGroup {
    fn kinds(&self) -> &'static [IntegrationKind] {
        KINDS
    }

    async fn test_connection(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<()> {
        Some(match ctx.kind {
            IntegrationKind::HomeAssistant => homeassistant::test_connection(rt, ctx).await,
            IntegrationKind::Thingport => thingport::test_connection(rt, ctx).await,
            IntegrationKind::Ntfy => ntfy::test_connection(rt, ctx).await,
            IntegrationKind::Gotify => gotify::test_connection(rt, ctx).await,
            IntegrationKind::Nextcloud => nextcloud::test_connection(rt, ctx).await,
            IntegrationKind::Ical => ical::test_connection(rt, ctx).await,
            IntegrationKind::PaperlessNgx => paperless::test_connection(rt, ctx).await,
            IntegrationKind::Anchor => anchor::test_connection(rt, ctx).await,
            IntegrationKind::Umami => umami::test_connection(rt, ctx).await,
            IntegrationKind::ArchiveTeamWarrior => {
                archive_team_warrior::test_connection(rt, ctx).await
            }
            IntegrationKind::Wazuh => wazuh::test_connection(rt, ctx).await,
            IntegrationKind::SpeedtestTracker => speedtest::test_connection(rt, ctx).await,
            _ => return None,
        })
    }

    // ---- calendar ----------------------------------------------------------------------------

    async fn calendar_events(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        _include_unmonitored: bool,
    ) -> Handled<Vec<CalendarEvent>> {
        Some(match ctx.kind {
            IntegrationKind::Ical => ical::calendar_events(rt, ctx, start, end).await,
            IntegrationKind::Nextcloud => nextcloud::calendar_events(rt, ctx, start, end).await,
            IntegrationKind::HomeAssistant => {
                homeassistant::calendar_events(rt, ctx, start, end).await
            }
            _ => return None,
        })
    }

    // ---- notifications -----------------------------------------------------------------------

    async fn notifications(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<Notification>> {
        Some(match ctx.kind {
            IntegrationKind::Gotify => gotify::notifications(rt, ctx).await,
            IntegrationKind::Ntfy => ntfy::notifications(rt, ctx).await,
            IntegrationKind::Nextcloud => nextcloud::notifications(rt, ctx).await,
            IntegrationKind::Wazuh => wazuh::notifications(rt, ctx).await,
            _ => return None,
        })
    }

    // ---- search ------------------------------------------------------------------------------

    async fn search(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        query: &str,
    ) -> Handled<Vec<SearchResult>> {
        (ctx.kind == IntegrationKind::Thingport).then_some(())?;
        Some(thingport::search(rt, ctx, query).await)
    }

    // ---- smart home --------------------------------------------------------------------------

    async fn smart_home_snapshot(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        selection: &SmartHomeEntitySelection,
    ) -> Handled<SmartHomeSnapshot> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::snapshot(rt, ctx, selection).await)
    }

    async fn smart_home_catalog(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<SmartHomeCatalog> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::catalog(rt, ctx).await)
    }

    async fn smart_home_history(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        entity_ids: &[String],
        hours: u32,
        samples: u32,
    ) -> Handled<Vec<SmartHomeHistorySeries>> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::history_series(rt, ctx, entity_ids, hours, samples).await)
    }

    async fn smart_home_forecast(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        entity_id: &str,
        forecast_type: SmartHomeForecastType,
    ) -> Handled<Vec<SmartHomeForecastDay>> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::forecast(rt, ctx, entity_id, forecast_type).await)
    }

    async fn smart_home_execute_action(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        entity_id: &str,
        action: &SmartHomeAction,
    ) -> Handled<Option<SmartHomeActionReceipt>> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::execute_action(rt, ctx, entity_id, action).await)
    }

    async fn smart_home_subscribe(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        selection: &SmartHomeEntitySelection,
        patches: mpsc::UnboundedSender<SmartHomeEntityPatch>,
        cancel: watch::Receiver<bool>,
    ) -> Handled<()> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::subscribe(rt, ctx, selection, patches, cancel).await)
    }

    // ---- integration specific data -----------------------------------------------------------

    async fn integration_data(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        operation: &str,
        input: &Value,
    ) -> Handled<Value> {
        Some(match ctx.kind {
            IntegrationKind::HomeAssistant => homeassistant::data(rt, ctx, operation, input).await,
            IntegrationKind::Thingport => thingport::data(rt, ctx, operation, input).await,
            IntegrationKind::Gotify => gotify::data(rt, ctx, operation, input).await,
            IntegrationKind::PaperlessNgx => paperless::data(rt, ctx, operation, input).await,
            IntegrationKind::Anchor => anchor::data(rt, ctx, operation, input).await,
            IntegrationKind::Umami => umami::data(rt, ctx, operation, input).await,
            IntegrationKind::ArchiveTeamWarrior => {
                archive_team_warrior::data(rt, ctx, operation, input).await
            }
            IntegrationKind::Wazuh => wazuh::data(rt, ctx, operation, input).await,
            IntegrationKind::SpeedtestTracker => speedtest::data(rt, ctx, operation, input).await,
            _ => return None,
        })
    }

    fn stats_metrics(&self, kind: IntegrationKind) -> Vec<StatsMetric> {
        use super::{count_metric, stat_metric};
        use IntegrationKind as K;
        match kind {
            K::PaperlessNgx => vec![
                count_metric("documentsTotal", "Documents"),
                count_metric("documentsInbox", "Inbox"),
                count_metric("tagsCount", "Tags"),
                count_metric("correspondentsCount", "Correspondents"),
                count_metric("documentTypesCount", "Document types"),
            ],
            K::Thingport => vec![
                count_metric("modelCount", "Models"),
                count_metric("collectionCount", "Collections"),
                count_metric("categoryCount", "Categories"),
                count_metric("authorCount", "Authors"),
            ],
            K::SpeedtestTracker => vec![
                stat_metric("ping", "Latency", StatsUnit::Milliseconds),
                stat_metric("download", "Download", StatsUnit::BytesPerSecond),
                stat_metric("upload", "Upload", StatsUnit::BytesPerSecond),
            ],
            K::Ntfy | K::Gotify | K::Nextcloud => {
                vec![count_metric("messages", "Retrieved notifications")]
            }
            K::Umami => vec![count_metric("websites", "Websites")],
            K::ArchiveTeamWarrior => vec![
                count_metric("running", "Running"),
                count_metric("completed", "Completed"),
                count_metric("failed", "Failed"),
            ],
            K::HomeAssistant => vec![
                count_metric("entities", "Entities"),
                count_metric("unavailable", "Unavailable entities"),
                count_metric("lightsOn", "Lights on"),
                count_metric("peopleHome", "People home"),
            ],
            K::Anchor => vec![count_metric("notes", "Retrieved notes")],
            K::Wazuh => vec![
                count_metric("agents", "Agents"),
                count_metric("activeAgents", "Active agents"),
                count_metric("disconnectedAgents", "Disconnected agents"),
                count_metric("alerts24h", "Alerts (24h)"),
                count_metric("highAlerts24h", "High alerts (24h)"),
                count_metric("criticalAlerts24h", "Critical alerts (24h)"),
            ],
            _ => Vec::new(),
        }
    }

    async fn stats(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<StatsValues> {
        use IntegrationKind as K;
        Some(match ctx.kind {
            K::PaperlessNgx => paperless::stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    (
                        "documentsTotal".into(),
                        StatsValue::Number(s.documents_total),
                    ),
                    (
                        "documentsInbox".into(),
                        StatsValue::Number(s.documents_inbox),
                    ),
                    ("tagsCount".into(), StatsValue::Number(s.tags_count)),
                    (
                        "correspondentsCount".into(),
                        StatsValue::Number(s.correspondents_count),
                    ),
                    (
                        "documentTypesCount".into(),
                        StatsValue::Number(s.document_types_count),
                    ),
                ])
            }),
            K::Thingport => thingport::library_stats(rt, ctx).await.map(|s| {
                StatsValues::from([
                    ("modelCount".into(), StatsValue::Number(s.model_count)),
                    (
                        "collectionCount".into(),
                        StatsValue::Number(s.collection_count),
                    ),
                    ("categoryCount".into(), StatsValue::Number(s.category_count)),
                    ("authorCount".into(), StatsValue::Number(s.author_count)),
                ])
            }),
            K::SpeedtestTracker => speedtest::latest_result(rt, ctx)
                .await
                .map(|opt| match opt {
                    Some(data) => StatsValues::from([
                        (
                            "ping".into(),
                            data.ping.map_or(StatsValue::Null, StatsValue::Number),
                        ),
                        (
                            "download".into(),
                            data.download_bits
                                .map_or(StatsValue::Null, |b| StatsValue::Number(b as f64 / 8.0)),
                        ),
                        (
                            "upload".into(),
                            data.upload_bits
                                .map_or(StatsValue::Null, |b| StatsValue::Number(b as f64 / 8.0)),
                        ),
                    ]),
                    None => StatsValues::from([
                        ("ping".into(), StatsValue::Null),
                        ("download".into(), StatsValue::Null),
                        ("upload".into(), StatsValue::Null),
                    ]),
                }),
            K::Ntfy | K::Gotify | K::Nextcloud => self.notifications(rt, ctx).await?.map(|items| {
                StatsValues::from([("messages".into(), StatsValue::Number(items.len() as f64))])
            }),
            K::Umami => umami::websites(rt, ctx).await.map(|sites| {
                StatsValues::from([("websites".into(), StatsValue::Number(sites.len() as f64))])
            }),
            K::ArchiveTeamWarrior => archive_team_warrior::status(rt, ctx).await.map(|status| {
                StatsValues::from([
                    (
                        "running".into(),
                        StatsValue::Number(status.counts.running as f64),
                    ),
                    (
                        "completed".into(),
                        StatsValue::Number(status.counts.completed as f64),
                    ),
                    (
                        "failed".into(),
                        StatsValue::Number(status.counts.failed as f64),
                    ),
                ])
            }),
            K::HomeAssistant => homeassistant::entity_stats(rt, ctx).await.map(|val| {
                let num = |k: &str| {
                    val.get(k)
                        .and_then(Value::as_f64)
                        .map_or(StatsValue::Null, StatsValue::Number)
                };
                StatsValues::from([
                    ("entities".into(), num("entities")),
                    ("unavailable".into(), num("unavailable")),
                    ("lightsOn".into(), num("lightsOn")),
                    ("peopleHome".into(), num("peopleHome")),
                ])
            }),
            K::Anchor => anchor::list_notes(rt, ctx, &anchor::NotesListInput::default())
                .await
                .map(|notes| {
                    StatsValues::from([("notes".into(), StatsValue::Number(notes.len() as f64))])
                }),
            K::Wazuh => {
                let (agents_res, summary_res) = tokio::join!(
                    wazuh::agents_overview(rt, ctx),
                    wazuh::security_summary(rt, ctx, "24h"),
                );
                agents_res.map(|agents| {
                    let alerts = summary_res.ok().and_then(|s| s.alerts);
                    StatsValues::from([
                        (
                            "agents".into(),
                            StatsValue::Number(agents.counts.total as f64),
                        ),
                        (
                            "activeAgents".into(),
                            StatsValue::Number(agents.counts.active as f64),
                        ),
                        (
                            "disconnectedAgents".into(),
                            StatsValue::Number(agents.counts.disconnected as f64),
                        ),
                        (
                            "alerts24h".into(),
                            alerts
                                .as_ref()
                                .map_or(StatsValue::Null, |a| StatsValue::Number(a.total as f64)),
                        ),
                        (
                            "highAlerts24h".into(),
                            alerts
                                .as_ref()
                                .map_or(StatsValue::Null, |a| StatsValue::Number(a.high as f64)),
                        ),
                        (
                            "criticalAlerts24h".into(),
                            alerts.as_ref().map_or(StatsValue::Null, |a| {
                                StatsValue::Number(a.critical as f64)
                            }),
                        ),
                    ])
                })
            }
            _ => return None,
        })
    }

    // ---- legacy adapters ---------------------------------------------------------------------

    async fn legacy_calendar_events(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Handled<Vec<LegacyCalendarEvent>> {
        matches!(
            ctx.kind,
            IntegrationKind::Ical | IntegrationKind::Nextcloud | IntegrationKind::HomeAssistant
        )
        .then_some(())?;
        let (start, end) = match legacy_window(start, end) {
            Ok(window) => window,
            Err(err) => return Some(Err(err)),
        };
        let events = self.calendar_events(rt, ctx, start, end, false).await?;
        Some(events.map(|events| {
            events
                .into_iter()
                .enumerate()
                .map(|(index, event)| legacy_calendar_event(ctx, index, event))
                .collect()
        }))
    }

    async fn legacy_smart_home_entities(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<LegacySmartHomeEntity>> {
        is_home_assistant(ctx).then_some(())?;
        Some(homeassistant::legacy_entities(rt, ctx).await)
    }

    async fn legacy_smart_home_action(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
        entity_id: &str,
        service: &str,
    ) -> Handled<()> {
        is_home_assistant(ctx).then_some(())?;
        let Some(action) = homeassistant::action_for_service(service) else {
            return Some(Err(HomarrError::BadRequest(format!(
                "Service '{service}' is not supported for {entity_id}"
            ))));
        };
        let result = self
            .smart_home_execute_action(rt, ctx, entity_id, &action)
            .await?;
        Some(result.map(|_| ()))
    }

    async fn legacy_notifications(
        &self,
        rt: &IntegrationRuntime,
        ctx: &IntegrationContext,
    ) -> Handled<Vec<NotificationItem>> {
        let notifications = self.notifications(rt, ctx).await?;
        Some(notifications.map(|items| items.into_iter().map(legacy_notification).collect()))
    }
}

// `Duration` is only used by the iCal default window helper; keep the import honest for rustc.
const _: fn() -> Duration = Duration::zero;
