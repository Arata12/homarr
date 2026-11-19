use homarr_definitions::WidgetKind;
use html_escape::{encode_double_quoted_attribute as encode_attr, encode_text};
use serde_json::Value;
use url::Url;

use crate::registry::get_widget_metadata;

/// Filters unsafe URL schemes (`javascript:`, `data:text/html`, protocol-relative `//`, quotes)
/// in rendered anchors/images/frames.
pub fn safe_url(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
    {
        return "#";
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || trimmed.starts_with('/') {
        trimmed
    } else {
        "#"
    }
}

fn host_port_key(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    Some(match url.port_or_known_default() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    })
}

fn is_same_origin_url(safe: &str, self_host_or_origin: Option<&str>) -> bool {
    let Ok(target) = Url::parse(safe) else {
        return false;
    };
    let check_against = |candidate: &str| -> bool {
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            return false;
        }
        if let Ok(origin_url) = Url::parse(trimmed) {
            return target.origin() == origin_url.origin();
        }
        // Host[:port] without scheme (e.g., from Host / X-Forwarded-Host header): compare host+port
        if let Ok(synthetic) = Url::parse(&format!("{}://{trimmed}", target.scheme())) {
            return host_port_key(&target) == host_port_key(&synthetic);
        }
        false
    };

    if self_host_or_origin.is_some_and(check_against) {
        return true;
    }
    if let Ok(base) = std::env::var("BASE_URL") {
        if check_against(&base) {
            return true;
        }
    }
    false
}

fn safe_hex_color(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with('#')
        && (trimmed.len() == 4 || trimmed.len() == 7)
        && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        trimmed
    } else {
        "#339af0"
    }
}

fn format_compact_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_speed(bps: u64) -> String {
    if bps >= 1_000_000_000 {
        format!("{:.1} GB/s", bps as f64 / 1_000_000_000.0)
    } else if bps >= 1_000_000 {
        format!("{:.1} MB/s", bps as f64 / 1_000_000.0)
    } else if bps >= 1_000 {
        format!("{:.0} KB/s", bps as f64 / 1_000.0)
    } else {
        format!("{bps} B/s")
    }
}

fn format_si_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000_000 {
        format!("{:.1} TB", bytes as f64 / 1_000_000_000_000.0)
    } else if bytes >= 1_000_000_000 {
        format!("{:.0} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{bytes} B")
    }
}

fn format_gib(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_073_741_824.0)
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        let mins = (secs % 3_600) / 60;
        format!("{hours}h {mins}m")
    }
}

fn format_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn weather_visual(code: i64, is_day: bool) -> (&'static str, &'static str) {
    match code {
        0 => {
            if is_day {
                (
                    "Clear sky",
                    r#"<svg class="weather-svg weather-anim-sun" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32l1.41 1.41M2 12h2m16 0h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" /></svg>"#,
                )
            } else {
                (
                    "Clear night",
                    r#"<svg class="weather-svg weather-anim-moon" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3c.132 0 .263 0 .393 0a7.5 7.5 0 0 0 7.92 12.446a9 9 0 1 1 -8.313 -12.454z" /></svg>"#,
                )
            }
        }
        1..=3 | 45 | 48 => (
            if code <= 2 { "Partly cloudy" } else { "Cloudy" },
            r#"<svg class="weather-svg weather-anim-cloud" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6.657 18c-2.572 0 -4.657 -2.007 -4.657 -4.483c0 -2.475 2.085 -4.482 4.657 -4.482c.393 -1.762 1.794 -3.2 3.675 -3.773c1.88 -.572 3.956 -.193 5.444 1c1.488 1.19 2.162 3.007 1.77 4.769h.99c1.913 0 3.464 1.56 3.464 3.486c0 1.927 -1.551 3.487 -3.465 3.487h-11.878" /></svg>"#,
        ),
        51..=67 | 80..=82 => (
            "Rain",
            r#"<svg class="weather-svg weather-anim-rain" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 18a4.6 4.4 0 0 1 0 -9a5 4.5 0 0 1 11 2h1a3.5 3.5 0 0 1 0 7" /><path d="M11 13v2m0 3v2m4 -5v2m0 3v2" /></svg>"#,
        ),
        71..=77 | 85..=86 => (
            "Snow",
            r#"<svg class="weather-svg weather-anim-snow" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 18a4.6 4.4 0 0 1 0 -9a5 4.5 0 0 1 11 2h1a3.5 3.5 0 0 1 0 7" /><path d="M11 15v.01m0 3v.01m0 3v.01m4 -4v.01m0 3v.01" /></svg>"#,
        ),
        95..=99 => (
            "Thunderstorm",
            r#"<svg class="weather-svg weather-anim-storm" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 18a4.6 4.4 0 0 1 0 -9a5 4.5 0 0 1 11 2h1a3.5 3.5 0 0 1 0 7h-1" /><path d="M13 14l-2 4l3 0l-2 4" /></svg>"#,
        ),
        _ => (
            "Overcast",
            r#"<svg class="weather-svg weather-anim-cloud" viewBox="0 0 24 24" width="44" height="44" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6.657 18c-2.572 0 -4.657 -2.007 -4.657 -4.483c0 -2.475 2.085 -4.482 4.657 -4.482c.393 -1.762 1.794 -3.2 3.675 -3.773c1.88 -.572 3.956 -.193 5.444 1c1.488 1.19 2.162 3.007 1.77 4.769h.99c1.913 0 3.464 1.56 3.464 3.486c0 1.927 -1.551 3.487 -3.465 3.487h-11.878" /></svg>"#,
        ),
    }
}

fn widget_header_icon(kind: WidgetKind) -> &'static str {
    use WidgetKind as W;
    match kind {
        W::Downloads => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2 -2v-2" /><path d="M7 11l5 5l5 -5" /><path d="M12 4l0 12" /></svg>"#
        }
        W::MediaServer => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M7 4v16l13 -8z" /></svg>"#
        }
        W::HealthMonitoring | W::SystemResources | W::SystemDisks => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12h4l3 8l4 -16l3 8h4" /></svg>"#
        }
        W::Calendar => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7a2 2 0 0 1 2 -2h12a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2v-12z" /><path d="M16 3v4" /><path d="M8 3v4" /><path d="M4 11h16" /></svg>"#
        }
        W::Bookmarks => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 7v14l-6 -4l-6 4v-14a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4z" /></svg>"#
        }
        W::SmartHomeEntityState | W::SmartHomeExecuteAutomation | W::SmartHomeControls => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l-2 0l9 -9l9 9l-2 0" /><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-7" /><path d="M10 12h4v4h-4z" /></svg>"#
        }
        W::DockerContainers => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l8 4.5v9l-8 4.5l-8 -4.5v-9z" /></svg>"#
        }
        W::RssFeed => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" /><path d="M4 4a16 16 0 0 1 16 16" /><path d="M4 11a9 9 0 0 1 9 9" /></svg>"#
        }
        _ => {
            r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h6v6h-6z" /><path d="M14 4h6v6h-6z" /><path d="M4 14h6v6h-6z" /><path d="M14 14h6v6h-6z" /></svg>"#
        }
    }
}

/// Renders an XSS-safe HTML card body for any `WidgetKind`.
pub fn render_widget_html(
    kind: WidgetKind,
    item_id: &str,
    options: &Value,
    data: &Value,
) -> String {
    render_widget_html_with_origin(kind, item_id, options, data, None)
}

/// Renders an XSS-safe HTML card body for any `WidgetKind`, stripping `allow-same-origin`
/// on `iframe` embeds that target `self_origin` (origin URL or `host[:port]`).
pub fn render_widget_html_with_origin(
    kind: WidgetKind,
    item_id: &str,
    options: &Value,
    data: &Value,
    self_origin: Option<&str>,
) -> String {
    let meta = get_widget_metadata(kind);
    let safe_id = encode_attr(item_id);
    let custom_title = options
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());
    let title = encode_text(custom_title.unwrap_or(&meta.name));

    if data
        .get("missingIntegration")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return format!(
            r#"<div class="widget-shell widget-{kind_str}" data-item-id="{safe_id}">
  <div class="widget-header">
    <span class="widget-header-title">{icon}<span>{title}</span></span>
    <span class="badge muted">No Integration</span>
  </div>
  <div class="widget-empty">Assign a compatible integration in board edit mode to display live telemetry.</div>
</div>"#,
            kind_str = encode_attr(kind.as_str()),
            icon = widget_header_icon(kind),
        );
    }

    let body = match kind {
        WidgetKind::App => render_app(options, data),
        WidgetKind::Bookmarks => render_bookmarks(options),
        WidgetKind::Clock => render_clock(options, data),
        WidgetKind::Weather => render_weather(options, data),
        WidgetKind::AirQuality => render_air_quality(data),
        WidgetKind::Notebook => render_notebook(options),
        WidgetKind::Countdown => render_countdown(options),
        WidgetKind::Timer => render_timer(options),
        WidgetKind::Iframe => render_iframe(options, self_origin),
        WidgetKind::Video => render_video(options),
        WidgetKind::RssFeed => render_rss(data),
        WidgetKind::StockPrice => render_stock(data),
        WidgetKind::MinecraftServerStatus => render_minecraft(data),
        WidgetKind::Releases => render_release(data),
        WidgetKind::DockerContainers => render_docker(data),
        WidgetKind::Downloads => render_downloads(data),
        WidgetKind::MediaServer => render_media_sessions(data),
        WidgetKind::MediaRequestsRequestList | WidgetKind::MediaRequestsRequestStats => {
            render_media_requests(kind, data)
        }
        WidgetKind::Calendar => render_calendar(data),
        WidgetKind::DnsHoleSummary | WidgetKind::DnsHoleControls => render_dns_hole(kind, data),
        WidgetKind::HealthMonitoring | WidgetKind::SystemResources | WidgetKind::SystemDisks => {
            render_health(kind, data)
        }
        WidgetKind::SmartHomeEntityState
        | WidgetKind::SmartHomeExecuteAutomation
        | WidgetKind::SmartHomeControls => render_smart_home(data),
        WidgetKind::Notifications => render_notifications(data),
        WidgetKind::IndexerManager => render_indexers(data),
        WidgetKind::Ups => render_ups(data),
        WidgetKind::MediaMissing => render_media_missing(data),
        WidgetKind::MediaReleases => render_media_releases(data),
        WidgetKind::MediaTranscoding => render_media_transcoding(data),
        WidgetKind::Firewall => render_firewall(data),
        WidgetKind::Vpn => render_vpn(data),
        WidgetKind::Traefik => render_traefik(data),
        WidgetKind::NetworkControllerSummary | WidgetKind::NetworkControllerStatus => {
            render_network_controller(data)
        }
        WidgetKind::AnchorNote => render_anchor_note(data),
        WidgetKind::Stats => render_stats(data),
        WidgetKind::CustomApi => render_custom_api(data),
        WidgetKind::Assistant => render_assistant(options),
        _ if data.get("error").and_then(|v| v.as_str()).is_some() => render_generic(data),
        _ if data.get("stats").is_some() => render_stats_with_extras(kind, data),
        _ => render_generic(data),
    };

    let show_header = !matches!(
        kind,
        WidgetKind::App
            | WidgetKind::Clock
            | WidgetKind::Weather
            | WidgetKind::DnsHoleSummary
            | WidgetKind::AirQuality
            | WidgetKind::Iframe
            | WidgetKind::Countdown
            | WidgetKind::Timer
    );

    if show_header {
        format!(
            r#"<div class="widget-shell widget-{kind_str}" data-item-id="{safe_id}">
  <div class="widget-header">
    <span class="widget-header-title">{icon}<span>{title}</span></span>
  </div>
  <div class="widget-body">{body}</div>
</div>"#,
            kind_str = encode_attr(kind.as_str()),
            icon = widget_header_icon(kind),
        )
    } else {
        format!(
            r#"<div class="widget-shell widget-shell-flush widget-{kind_str}" data-item-id="{safe_id}">{body}</div>"#,
            kind_str = encode_attr(kind.as_str()),
        )
    }
}

fn render_app(options: &Value, data: &Value) -> String {
    let Some(app) = data.get("app").filter(|v| !v.is_null()) else {
        return r#"<div class="widget-empty">Select an App</div>"#.to_string();
    };
    let name_raw = app.get("name").and_then(|v| v.as_str()).unwrap_or("App");
    let name = encode_text(name_raw);
    let desc_raw = app
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let icon = safe_url(
        app.get("iconUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("/logo/homarr.svg"),
    );
    let href = safe_url(app.get("href").and_then(|v| v.as_str()).unwrap_or("#"));
    let ping_url = app
        .get("pingUrl")
        .or_else(|| app.get("href"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let show_title = options
        .get("showTitle")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let ping_enabled = options
        .get("pingEnabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let ping_badge = if ping_enabled && !ping_url.is_empty() {
        let status_cls = match data
            .get("ping")
            .and_then(|p| p.get("statusCode"))
            .and_then(|s| s.as_u64())
        {
            Some(code) if code < 400 => "ping-dot up",
            Some(_) => "ping-dot down",
            None => "ping-dot unknown",
        };
        format!(r#"<span class="{status_cls}" title="Service status"></span>"#)
    } else {
        String::new()
    };

    let title_html = if show_title {
        format!(r#"<div class="app-name">{name}</div>"#)
    } else {
        String::new()
    };

    let title_raw = if desc_raw.is_empty() {
        name_raw.to_string()
    } else {
        format!("{name_raw} — {desc_raw}")
    };

    format!(
        r#"<a class="app-tile" href="{href}" target="_blank" rel="noopener noreferrer" data-ping-url="{ping}" data-app-id="{app_id}" title="{title_attr}">
  <img class="app-icon" src="{icon}" alt="{alt_name}" loading="lazy" onerror="this.onerror=null;this.src='/logo/homarr.svg'" />
  {title_html}
  {ping_badge}
</a>"#,
        href = encode_attr(href),
        icon = encode_attr(icon),
        ping = encode_attr(ping_url),
        app_id = encode_attr(app.get("id").and_then(|v| v.as_str()).unwrap_or("")),
        title_attr = encode_attr(&title_raw),
        alt_name = encode_attr(name_raw),
    )
}

fn render_bookmarks(options: &Value) -> String {
    let items = options
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return r#"<div class="widget-empty">No bookmarks configured</div>"#.to_string();
    }
    let mut html = String::from(r#"<div class="bookmark-chips">"#);
    for item in items.iter().take(20) {
        let name = encode_text(item.get("name").and_then(|v| v.as_str()).unwrap_or("Link"));
        let href = encode_attr(safe_url(
            item.get("href").and_then(|v| v.as_str()).unwrap_or("#"),
        ));
        let icon_url = item
            .get("iconUrl")
            .and_then(|v| v.as_str())
            .map(safe_url)
            .filter(|u| *u != "#");
        let icon_html = match icon_url {
            Some(url) => format!(
                r#"<img class="bookmark-chip-icon" src="{}" alt="" loading="lazy" onerror="this.style.display='none'" />"#,
                encode_attr(url)
            ),
            None => r#"<svg class="ui-icon" viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M9 15l6 -6" /><path d="M11 6l.463 -.536a5 5 0 0 1 7.071 7.072l-.534 .464" /><path d="M13 18l-.397 .534a5.068 5.068 0 0 1 -7.127 0a4.972 4.972 0 0 1 0 -7.071l.524 -.463" /></svg>"#.to_string(),
        };
        html.push_str(&format!(
            r#"<a class="bookmark-chip" href="{href}" target="_blank" rel="noopener noreferrer">
  {icon_html}
  <span class="bookmark-chip-name">{name}</span>
  <svg class="bookmark-chip-ext" viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 6h-6a2 2 0 0 0 -2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-6" /><path d="M11 13l9 -9" /><path d="M15 4h5v5" /></svg>
</a>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_clock(options: &Value, data: &Value) -> String {
    let is_24 = options
        .get("is24HourFormat")
        .or_else(|| options.get("is24Hour"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let time = if is_24 {
        data.get("time24")
            .and_then(|v| v.as_str())
            .unwrap_or("--:--")
    } else {
        data.get("time12")
            .and_then(|v| v.as_str())
            .unwrap_or("--:--")
    };
    let show_date = options
        .get("showDate")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let date = if show_date {
        data.get("date").and_then(|v| v.as_str()).unwrap_or("")
    } else {
        ""
    };
    let title = options
        .get("customTitle")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let custom_title_html = if title.trim().is_empty() {
        String::new()
    } else {
        format!(r#"<div class="clock-sub">{}</div>"#, encode_text(title))
    };
    let date_html = if date.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="clock-date">{}</div>"#, encode_text(date))
    };

    format!(
        r#"<div class="clock-widget" data-24h="{is_24}">
  {custom_title_html}
  <div class="clock-time">{time}</div>
  {date_html}
</div>"#,
        time = encode_text(time),
    )
}

fn render_weather(options: &Value, data: &Value) -> String {
    if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
        return format!(r#"<div class="widget-empty">{}</div>"#, encode_text(err));
    }
    let city = encode_text(
        data.get("locationName")
            .and_then(|v| v.as_str())
            .unwrap_or("Weather"),
    );
    let current = data.get("weather").and_then(|w| w.get("current"));
    let temp = current
        .and_then(|c| c.get("temperature2m"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let wind = current
        .and_then(|c| c.get("windSpeed10m"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let code = current
        .and_then(|c| c.get("weatherCode"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let is_day = current
        .and_then(|c| c.get("isDay"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let is_f = options
        .get("isFormatFahrenheit")
        .or_else(|| options.get("isFahrenheit"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let convert_temp = |c: f64| if is_f { c * 9.0 / 5.0 + 32.0 } else { c };
    let unit = if is_f { "°F" } else { "°C" };
    let display_temp = format!("{:.1}{unit}", convert_temp(temp));
    let (label, svg_icon) = weather_visual(code, is_day);

    let hi_lo = data
        .get("weather")
        .and_then(|w| w.get("daily"))
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .and_then(|day| {
            let max_c = day.get("temperature2mMax").and_then(|v| v.as_f64())?;
            let min_c = day.get("temperature2mMin").and_then(|v| v.as_f64())?;
            Some(format!(
                r#"<span>H {:.0}° / L {:.0}°</span>"#,
                convert_temp(max_c),
                convert_temp(min_c)
            ))
        })
        .unwrap_or_default();

    format!(
        r#"<div class="weather-widget">
  <div class="weather-top">
    <div class="weather-temp-group">
      <div class="weather-temp">{display_temp}</div>
      <div class="weather-cond">{label}</div>
    </div>
    <div class="weather-icon-wrap">{svg_icon}</div>
  </div>
  <div class="weather-meta">
    <span class="weather-city">{city}</span>
    <span class="weather-pills">{hi_lo}<span>Wind {wind:.0} km/h</span></span>
  </div>
</div>"#
    )
}

fn render_air_quality(data: &Value) -> String {
    let aq = data.get("airQuality");
    let eu = aq
        .and_then(|v| v.get("europeanAqi"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let pm25 = aq
        .and_then(|v| v.get("pm25").or_else(|| v.get("pm2_5")))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let quality = if eu <= 20.0 {
        "Good"
    } else if eu <= 40.0 {
        "Fair"
    } else if eu <= 60.0 {
        "Moderate"
    } else {
        "Poor"
    };
    format!(
        r#"<div class="stats-grid" style="height:100%;align-content:center">
  <div class="stat-pill"><span class="label">EU AQI ({quality})</span><strong>{eu:.0}</strong></div>
  <div class="stat-pill"><span class="label">PM2.5</span><strong>{pm25:.1} µg/m³</strong></div>
</div>"#
    )
}

fn render_inline_md(escaped: &str) -> String {
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped;
    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_code = rest.find('`');
        let pick_bold = match (next_bold, next_code) {
            (Some(b), Some(c)) => Some(b <= c),
            (Some(_), None) => Some(true),
            (None, Some(_)) => Some(false),
            (None, None) => None,
        };
        match pick_bold {
            Some(true) => {
                let b = next_bold.unwrap_or(0);
                out.push_str(&rest[..b]);
                let after = &rest[b + 2..];
                if let Some(end) = after.find("**") {
                    out.push_str("<strong>");
                    out.push_str(&after[..end]);
                    out.push_str("</strong>");
                    rest = &after[end + 2..];
                } else {
                    out.push_str("**");
                    rest = after;
                }
            }
            Some(false) => {
                let c = next_code.unwrap_or(0);
                out.push_str(&rest[..c]);
                let after = &rest[c + 1..];
                if let Some(end) = after.find('`') {
                    out.push_str("<code>");
                    out.push_str(&after[..end]);
                    out.push_str("</code>");
                    rest = &after[end + 1..];
                } else {
                    out.push('`');
                    rest = after;
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// Finds the `>` that ends a tag, skipping quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '>') => return Some(i),
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if q == c => quote = None,
            _ => {}
        }
    }
    None
}

/// Value of the first attribute named `wanted` in the attribute part of a tag.
fn attr_value<'a>(attrs: &'a str, wanted: &str) -> Option<&'a str> {
    let mut s = attrs;
    loop {
        s = s.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if s.is_empty() {
            return None;
        }
        let name_end = s
            .find(|c: char| c.is_ascii_whitespace() || c == '=' || c == '/')
            .unwrap_or(s.len());
        let name = &s[..name_end];
        s = s[name_end..].trim_start();
        let mut value = None;
        if let Some(v) = s.strip_prefix('=') {
            let v = v.trim_start();
            let (val, next) = match v.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let body = &v[1..];
                    let e = body.find(q).unwrap_or(body.len());
                    (&body[..e], body.get(e + 1..).unwrap_or(""))
                }
                _ => {
                    let e = v.find(|c: char| c.is_ascii_whitespace()).unwrap_or(v.len());
                    (&v[..e], &v[e..])
                }
            };
            value = Some(val);
            s = next;
        }
        if name.eq_ignore_ascii_case(wanted) {
            return value;
        }
    }
}

/// Allowlist sanitizer for notebook HTML (the original stores Tiptap HTML). Keeps basic
/// formatting tags without attributes, except `a[href]` with an http(s)/mailto URL; drops
/// script-like elements with their content and every other tag (keeping its text).
fn sanitize_notebook_html(input: &str) -> String {
    const ALLOWED: &str = "p br strong em u s h1 h2 h3 h4 h5 h6 ul ol li blockquote code pre a hr \
                           table thead tbody tr th td";
    const DROP_WITH_CONTENT: &str =
        "script style iframe object embed template noscript textarea title svg math";
    let mut out = String::with_capacity(input.len());
    let mut open: Vec<&str> = Vec::new();
    let mut rest = input;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt].replace('>', "&gt;"));
        let after = &rest[lt + 1..];
        if let Some(comment) = after.strip_prefix("!--") {
            rest = comment.find("-->").map_or("", |i| &comment[i + 3..]);
            continue;
        }
        let closing = after.starts_with('/');
        let name_src = if closing { &after[1..] } else { after };
        if !name_src.starts_with(|c: char| c.is_ascii_alphabetic()) {
            if closing || after.starts_with(['!', '?']) {
                // Doctype, processing instruction or bogus comment.
                rest = after.find('>').map_or("", |i| &after[i + 1..]);
            } else {
                out.push_str("&lt;");
                rest = after;
            }
            continue;
        }
        let name_len = name_src
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(name_src.len());
        let name = name_src[..name_len].to_ascii_lowercase();
        let Some(end) = tag_end(&name_src[name_len..]) else {
            // Unterminated tag: drop the remainder.
            rest = "";
            break;
        };
        let attrs = &name_src[name_len..name_len + end];
        rest = &name_src[name_len + end + 1..];
        let allowed = ALLOWED.split_whitespace().find(|t| *t == name);
        if closing {
            if let Some(pos) = allowed.and_then(|tag| open.iter().rposition(|t| *t == tag)) {
                for t in open.drain(pos..).rev() {
                    out.push_str("</");
                    out.push_str(t);
                    out.push('>');
                }
            }
            continue;
        }
        if DROP_WITH_CONTENT.split(' ').any(|t| t == name) {
            let lower = rest.to_ascii_lowercase();
            rest = match lower.find(&format!("</{name}")) {
                Some(i) => rest[i..].find('>').map_or("", |j| &rest[i + j + 1..]),
                None => "",
            };
            continue;
        }
        let Some(tag) = allowed else {
            continue;
        };
        out.push('<');
        out.push_str(tag);
        if tag == "a" {
            if let Some(href) = attr_value(attrs, "href").map(str::trim) {
                let lower = href.to_ascii_lowercase();
                if ["http://", "https://", "mailto:"]
                    .iter()
                    .any(|p| lower.starts_with(p))
                {
                    // `&` stays as written: entities like `&amp;` are valid inside attributes.
                    let href = href
                        .replace('"', "&quot;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    out.push_str(&format!(
                        r#" href="{href}" target="_blank" rel="noopener noreferrer""#
                    ));
                }
            }
        }
        out.push('>');
        if tag != "br" && tag != "hr" {
            open.push(tag);
        }
    }
    out.push_str(&rest.replace('>', "&gt;"));
    for t in open.into_iter().rev() {
        out.push_str("</");
        out.push_str(t);
        out.push('>');
    }
    out
}

fn render_notebook(options: &Value) -> String {
    let content = options
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if content.trim().is_empty() {
        return r#"<div class="widget-empty">Empty note</div>"#.to_string();
    }
    // Content saved by the original (Tiptap) is HTML; older port content is markdown-like text.
    if content.trim_start().starts_with('<') {
        return format!(
            r#"<div class="notebook-body">{}</div>"#,
            sanitize_notebook_html(content)
        );
    }
    let mut html = String::from(r#"<div class="notebook-body">"#);
    let mut in_list = false;
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        let is_list_item = trimmed.starts_with("- ") || trimmed.starts_with("* ");
        if !is_list_item && in_list {
            html.push_str("</ul>");
            in_list = false;
        }
        if trimmed.is_empty() {
            continue;
        }
        if let Some(h3) = trimmed.strip_prefix("### ") {
            let esc = encode_text(h3);
            html.push_str(&format!("<h6>{}</h6>", render_inline_md(&esc)));
        } else if let Some(h2) = trimmed.strip_prefix("## ") {
            let esc = encode_text(h2);
            html.push_str(&format!("<h5>{}</h5>", render_inline_md(&esc)));
        } else if let Some(h1) = trimmed.strip_prefix("# ") {
            let esc = encode_text(h1);
            html.push_str(&format!("<h4>{}</h4>", render_inline_md(&esc)));
        } else if let Some(task) = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            if !in_list {
                html.push_str(r#"<ul class="notebook-list">"#);
                in_list = true;
            }
            let esc = encode_text(task);
            html.push_str(&format!(
                r#"<li class="notebook-task"><input type="checkbox" disabled checked /><span>{}</span></li>"#,
                render_inline_md(&esc)
            ));
        } else if let Some(task) = trimmed.strip_prefix("- [ ] ") {
            if !in_list {
                html.push_str(r#"<ul class="notebook-list">"#);
                in_list = true;
            }
            let esc = encode_text(task);
            html.push_str(&format!(
                r#"<li class="notebook-task"><input type="checkbox" disabled /><span>{}</span></li>"#,
                render_inline_md(&esc)
            ));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if !in_list {
                html.push_str(r#"<ul class="notebook-list">"#);
                in_list = true;
            }
            let esc = encode_text(item);
            html.push_str(&format!("<li>{}</li>", render_inline_md(&esc)));
        } else {
            let esc = encode_text(trimmed);
            html.push_str(&format!("<p>{}</p>", render_inline_md(&esc)));
        }
    }
    if in_list {
        html.push_str("</ul>");
    }
    html.push_str("</div>");
    html
}

fn render_countdown(options: &Value) -> String {
    let title = encode_text(
        options
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Countdown"),
    );
    let date_raw = options.get("date").and_then(|v| v.as_str()).unwrap_or("");
    let date_attr = encode_attr(date_raw);
    let date = encode_text(date_raw);
    format!(
        r#"<div class="countdown-widget" data-target-date="{date_attr}">
  <div class="metric-sub">{title}</div>
  <div class="metric-big js-countdown">--d --h --m</div>
  <div class="metric-sub">{date}</div>
</div>"#
    )
}

fn render_timer(options: &Value) -> String {
    let title = encode_text(
        options
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Timer"),
    );
    let secs = options
        .get("durationSeconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(1500);
    format!(
        r#"<div class="timer-widget" data-duration="{secs}" data-remaining="{secs}">
  <div class="metric-sub">{title}</div>
  <div class="metric-big js-timer-display">{:02}:{:02}</div>
  <div class="timer-controls">
    <button type="button" class="btn btn-sm" onclick="window.toggleTimer(this)">Start</button>
    <button type="button" class="btn btn-sm" onclick="window.resetTimer(this)">Reset</button>
  </div>
</div>"#,
        secs / 60,
        secs % 60
    )
}

fn render_iframe(options: &Value, self_origin: Option<&str>) -> String {
    let raw = options
        .get("embedUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("about:blank");
    let safe = safe_url(raw);
    if safe == "#" {
        return r#"<div class="widget-empty">Invalid embed URL</div>"#.to_string();
    }
    // Same-origin (relative or matching Homarr's own origin) targets must never get
    // `allow-same-origin`, otherwise the framed page could reach Homarr's origin.
    let sandbox = if safe.starts_with('/') || is_same_origin_url(safe, self_origin) {
        "allow-scripts allow-forms"
    } else {
        "allow-scripts allow-same-origin allow-forms"
    };
    format!(
        r#"<iframe class="widget-iframe" src="{}" sandbox="{sandbox}" loading="lazy"></iframe>"#,
        encode_attr(safe)
    )
}

fn render_video(options: &Value) -> String {
    let raw = options
        .get("feedUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let safe = safe_url(raw);
    if safe == "#" {
        return r#"<div class="widget-empty">No video stream configured</div>"#.to_string();
    }
    format!(
        r#"<video class="widget-video" src="{}" controls muted playsinline></video>"#,
        encode_attr(safe)
    )
}

fn render_rss(data: &Value) -> String {
    let entries = data
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<div class="widget-empty">No RSS items available</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for e in entries.iter().take(10) {
        let title = encode_text(
            e.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled"),
        );
        let link = encode_attr(safe_url(
            e.get("link").and_then(|v| v.as_str()).unwrap_or("#"),
        ));
        html.push_str(&format!(
            r#"<li><a href="{link}" target="_blank" rel="noopener noreferrer">{title}</a></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_stock(data: &Value) -> String {
    if let Some(stock) = data.get("stock") {
        let sym = encode_text(stock.get("symbol").and_then(|v| v.as_str()).unwrap_or(""));
        let price = stock
            .get("regularMarketPrice")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let pct = stock
            .get("changePercent")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let cls = if pct >= 0.0 { "up" } else { "down" };
        return format!(
            r#"<div class="stock-widget"><strong>{sym}</strong> <span class="metric-big">${price:.2}</span> <span class="badge {cls}">{pct:+.2}%</span></div>"#
        );
    }
    r#"<div class="widget-empty">Stock quote unavailable</div>"#.to_string()
}

fn render_minecraft(data: &Value) -> String {
    let Some(st) = data.get("status") else {
        return r#"<div class="widget-empty">No server status</div>"#.to_string();
    };
    let online = st.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
    let host = encode_text(st.get("host").and_then(|v| v.as_str()).unwrap_or(""));
    let cur = st
        .get("playersOnline")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let max = st.get("playersMax").and_then(|v| v.as_u64()).unwrap_or(0);
    let badge = if online {
        r#"<span class="badge up">Online</span>"#
    } else {
        r#"<span class="badge down">Offline</span>"#
    };
    format!(
        r#"<div>{badge} <strong>{host}</strong><div class="metric-sub">Players: {cur} / {max}</div></div>"#
    )
}

fn render_release(data: &Value) -> String {
    let Some(rel) = data.get("release") else {
        return r#"<div class="widget-empty">Release data unavailable</div>"#.to_string();
    };
    let id = encode_text(rel.get("identifier").and_then(|v| v.as_str()).unwrap_or(""));
    let tag = encode_text(
        rel.get("latestRelease")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
    );
    let url = encode_attr(safe_url(
        rel.get("url").and_then(|v| v.as_str()).unwrap_or("#"),
    ));
    format!(
        r#"<div><strong>{id}</strong><div class="metric-big"><a href="{url}" target="_blank" rel="noopener noreferrer">{tag}</a></div></div>"#
    )
}

fn render_docker(data: &Value) -> String {
    let items = data
        .get("containers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return r#"<div class="widget-empty">No Docker containers reported</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for c in items.iter().take(12) {
        let name = encode_text(c.get("name").and_then(|v| v.as_str()).unwrap_or(""));
        let state = encode_text(c.get("state").and_then(|v| v.as_str()).unwrap_or(""));
        let cls = if state == "running" { "up" } else { "down" };
        html.push_str(&format!(
            r#"<li><span>{name}</span> <span class="badge {cls}">{state}</span></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_downloads(data: &Value) -> String {
    let Some(q) = data.get("queue") else {
        return r#"<div class="widget-empty">Queue unavailable</div>"#.to_string();
    };
    let items = q
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let dl = q
        .get("downloadSpeedBps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let ul = q
        .get("uploadSpeedBps")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut html = format!(
        r#"<div class="downloads-summary">
  <span class="speed-pill speed-dl">↓ {}</span>
  <span class="speed-pill speed-ul">↑ {}</span>
  <span class="muted" style="margin-left:auto;font-size:.76rem">{} active</span>
</div>
<div class="downloads-list">"#,
        format_speed(dl),
        format_speed(ul),
        items.len()
    );
    for item in items.iter().take(6) {
        let name_raw = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let name = encode_text(name_raw);
        let name_attr = encode_attr(name_raw);
        let state = encode_text(item.get("state").and_then(|v| v.as_str()).unwrap_or(""));
        let pct = (item.get("progress").and_then(|v| v.as_f64()).unwrap_or(0.0) * 100.0)
            .clamp(0.0, 100.0);
        let size = item
            .get("sizeBytes")
            .and_then(|v| v.as_u64())
            .map(format_si_bytes)
            .unwrap_or_default();
        let eta = item
            .get("etaSeconds")
            .and_then(|v| v.as_u64())
            .map(|e| format!(" · ETA {}", format_eta(e)))
            .unwrap_or_default();
        let fill_cls = if state == "seeding" {
            "progress-fill is-green"
        } else {
            "progress-fill"
        };
        html.push_str(&format!(
            r#"<div class="download-row">
  <div class="download-top">
    <span class="download-name" title="{name_attr}">{name}</span>
    <span class="badge">{state}</span>
  </div>
  <div class="progress-track"><div class="{fill_cls}" style="width:{pct:.1}%"></div></div>
  <div class="download-meta"><span>{size}{eta}</span><span>{pct:.0}%</span></div>
</div>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_media_sessions(data: &Value) -> String {
    let sessions = data
        .get("sessions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if sessions.is_empty() {
        return r#"<div class="widget-empty">Nothing playing right now</div>"#.to_string();
    }
    let mut html = String::from(r#"<div class="media-sessions">"#);
    for s in sessions.iter().take(6) {
        let item = encode_text(s.get("itemName").and_then(|v| v.as_str()).unwrap_or(""));
        let series = s
            .get("seriesName")
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(encode_text);
        let user = encode_text(s.get("username").and_then(|v| v.as_str()).unwrap_or(""));
        let client = encode_text(
            s.get("clientName")
                .and_then(|v| v.as_str())
                .unwrap_or("Player"),
        );
        let is_paused = s.get("isPaused").and_then(|v| v.as_bool()).unwrap_or(false);
        let pct = s
            .get("progressPercent")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            .clamp(0.0, 100.0);
        let status_badge = if is_paused {
            r#"<span class="badge muted">Paused</span>"#
        } else {
            r#"<span class="badge up">Playing</span>"#
        };
        let heading = match series {
            Some(sr) => {
                format!(r#"<div class="media-title">{sr}</div><div class="media-sub">{item}</div>"#)
            }
            None => format!(r#"<div class="media-title">{item}</div>"#),
        };
        html.push_str(&format!(
            r#"<div class="media-session-card">
  <div class="media-session-top">
    <div>{heading}</div>
    {status_badge}
  </div>
  <div class="progress-track"><div class="progress-fill" style="width:{pct:.1}%"></div></div>
  <div class="media-meta"><span>{user} &middot; {client}</span><span>{pct:.0}%</span></div>
</div>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_media_requests(kind: WidgetKind, data: &Value) -> String {
    if kind == WidgetKind::MediaRequestsRequestStats {
        let s = data.get("stats");
        let total = s
            .and_then(|v| v.get("total"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let pending = s
            .and_then(|v| v.get("pending"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let avail = s
            .and_then(|v| v.get("available"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return format!(
            r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">Total</span><strong>{total}</strong></div>
  <div class="stat-pill"><span class="label">Pending</span><strong>{pending}</strong></div>
  <div class="stat-pill"><span class="label">Available</span><strong>{avail}</strong></div>
</div>"#
        );
    }
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for item in items.iter().take(8) {
        let title = encode_text(item.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        let st = encode_text(item.get("status").and_then(|v| v.as_str()).unwrap_or(""));
        html.push_str(&format!(
            r#"<li><span>{title}</span> <span class="badge">{st}</span></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_calendar(data: &Value) -> String {
    let events = data
        .get("events")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if events.is_empty() {
        return r#"<div class="widget-empty">No upcoming releases</div>"#.to_string();
    }
    let mut html = String::from(r#"<div class="calendar-list">"#);
    for ev in events.iter().take(8) {
        let title = encode_text(ev.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        let sub = encode_text(ev.get("subTitle").and_then(|v| v.as_str()).unwrap_or(""));
        let start = encode_text(
            ev.get("startDate")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .get(..10)
                .unwrap_or(""),
        );
        let color = safe_hex_color(
            ev.get("indicatorColor")
                .and_then(|v| v.as_str())
                .unwrap_or("#339af0"),
        );
        html.push_str(&format!(
            r#"<div class="calendar-item">
  <span class="calendar-bar" style="background:{color}"></span>
  <div class="calendar-info">
    <div class="calendar-title">{title}</div>
    <div class="calendar-sub">{sub}</div>
  </div>
  <span class="badge">{start}</span>
</div>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_dns_hole(kind: WidgetKind, data: &Value) -> String {
    let first = data
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.get("summary"));
    let Some(s) = first else {
        return r#"<div class="widget-empty">DNS Hole metrics unavailable</div>"#.to_string();
    };
    let blocked = s
        .get("adsBlockedToday")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let queries = s
        .get("dnsQueriesToday")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let domains = s
        .get("domainsBeingBlocked")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let pct = s
        .get("adsPercentageToday")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let status = encode_text(
        s.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("enabled"),
    );

    let grid = format!(
        r#"<div class="dns-hole-grid">
  <div class="dns-tile dns-red">
    <svg class="dns-tile-icon" viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M13.252 20.601c-.408 .155 -.826 .288 -1.252 .399a12 12 0 0 1 -8.5 -15a12 12 0 0 0 8.5 -3a12 12 0 0 0 8.5 3a12 12 0 0 1 -.19 7.357" /><path d="M22 22l-5 -5" /><path d="M17 22l5 -5" /></svg>
    <div class="dns-tile-value">{}</div>
    <div class="dns-tile-label">Ads Blocked Today</div>
  </div>
  <div class="dns-tile dns-yellow">
    <svg class="dns-tile-icon" viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M17 17m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" /><path d="M7 7m-1 0a1 1 0 1 0 2 0a1 1 0 1 0 -2 0" /><path d="M6 18l12 -12" /></svg>
    <div class="dns-tile-value">{pct:.1}%</div>
    <div class="dns-tile-label">Queries Blocked</div>
  </div>
  <div class="dns-tile dns-cyan">
    <svg class="dns-tile-icon" viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10 10m-7 0a7 7 0 1 0 14 0a7 7 0 1 0 -14 0" /><path d="M21 21l-6 -6" /></svg>
    <div class="dns-tile-value">{}</div>
    <div class="dns-tile-label">DNS Queries Today</div>
  </div>
  <div class="dns-tile dns-green">
    <svg class="dns-tile-icon" viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0" /><path d="M3.6 9h16.8" /><path d="M3.6 15h16.8" /><path d="M11.5 3a17 17 0 0 0 0 18" /><path d="M12.5 3a17 17 0 0 1 0 18" /></svg>
    <div class="dns-tile-value">{}</div>
    <div class="dns-tile-label">Domains on Blocklist</div>
  </div>
</div>"#,
        format_compact_count(blocked),
        format_compact_count(queries),
        format_compact_count(domains),
    );

    if kind == WidgetKind::DnsHoleControls {
        format!(
            r#"<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px">
  <span class="metric-sub">Pi-hole / AdGuard</span>
  <span class="badge up">{status}</span>
</div>
{grid}"#
        )
    } else {
        grid
    }
}

fn render_donut_gauge(label: &str, pct: f64, sub: &str, color_var: &str) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    let circ = 150.796_f64;
    let offset = circ * (1.0 - clamped / 100.0);
    format!(
        r#"<div class="health-gauge">
  <div class="gauge-ring">
    <svg viewBox="0 0 64 64" width="60" height="60" aria-hidden="true">
      <circle class="gauge-track" cx="32" cy="32" r="24" />
      <circle class="gauge-arc" cx="32" cy="32" r="24" style="stroke:{color_var};stroke-dasharray:{circ:.1};stroke-dashoffset:{offset:.1}" />
    </svg>
    <span class="gauge-pct">{clamped:.0}%</span>
  </div>
  <div class="gauge-text">
    <div class="gauge-label">{label}</div>
    <div class="gauge-sub">{sub}</div>
  </div>
</div>"#,
        label = encode_text(label),
        sub = encode_text(sub),
    )
}

fn render_health(_kind: WidgetKind, data: &Value) -> String {
    let Some(h) = data.get("health") else {
        return r#"<div class="widget-empty">Health telemetry unavailable</div>"#.to_string();
    };
    let name = encode_text(
        h.get("systemName")
            .and_then(|v| v.as_str())
            .unwrap_or("Host"),
    );
    let uptime = h
        .get("uptimeSeconds")
        .and_then(|v| v.as_u64())
        .map(format_uptime)
        .unwrap_or_else(|| "-".to_string());
    let cpu = h.get("cpuPercent").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let cpu_sub = h
        .get("cpuTempC")
        .and_then(|v| v.as_f64())
        .map(|t| format!("{t:.0}°C"))
        .unwrap_or_else(|| "Load avg".to_string());
    let mem_used = h
        .get("memoryUsedBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mem_total = h
        .get("memoryTotalBytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1);
    let mem_pct = (mem_used as f64 / mem_total as f64) * 100.0;
    let mem_sub = format!("{} / {} GiB", format_gib(mem_used), format_gib(mem_total));

    let mut html = format!(
        r#"<div class="health-host-row">
  <strong>{name}</strong>
  <span class="badge up">Up {uptime}</span>
</div>
<div class="health-gauges">
  {}
  {}
</div>"#,
        render_donut_gauge("CPU", cpu, &cpu_sub, "var(--mantine-blue)"),
        render_donut_gauge("Memory", mem_pct, &mem_sub, "var(--mantine-violet)"),
    );

    if let Some(disks) = h.get("disks").and_then(|v| v.as_array()) {
        if !disks.is_empty() {
            html.push_str(r#"<div class="health-disks">"#);
            for d in disks.iter().take(2) {
                let mount = encode_text(
                    d.get("mountPoint")
                        .or_else(|| d.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("/"),
                );
                let used = d.get("usedBytes").and_then(|v| v.as_u64()).unwrap_or(0);
                let total = d
                    .get("totalBytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1)
                    .max(1);
                let pct = (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0);
                html.push_str(&format!(
                    r#"<div class="health-disk-row">
  <div class="download-meta"><span><code>{mount}</code></span><span>{pct:.0}%</span></div>
  <div class="progress-track"><div class="progress-fill is-cyan" style="width:{pct:.1}%"></div></div>
</div>"#
                ));
            }
            html.push_str("</div>");
        }
    }

    if let Some(nodes) = h.get("nodes").and_then(|v| v.as_array()) {
        if !nodes.is_empty() {
            html.push_str(r#"<div class="node-pills">"#);
            for n in nodes.iter().take(4) {
                let node_name = encode_text(n.get("name").and_then(|v| v.as_str()).unwrap_or(""));
                let st = n.get("status").and_then(|v| v.as_str()).unwrap_or("online");
                let dot_cls = if st == "online" { "up" } else { "down" };
                html.push_str(&format!(
                    r#"<span class="node-pill"><span class="dot {dot_cls}"></span>{node_name}</span>"#
                ));
            }
            html.push_str("</div>");
        }
    }

    html
}

fn render_smart_home(data: &Value) -> String {
    let entities = data
        .get("entities")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if entities.is_empty() {
        return r#"<div class="widget-empty">No smart home entities found</div>"#.to_string();
    }
    let mut html = String::from(r#"<div class="smart-home-grid">"#);
    for e in entities.iter().take(6) {
        let name = encode_text(
            e.get("friendlyName")
                .or_else(|| e.get("entityId"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
        );
        let st_raw = e.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let unit = e
            .get("unitOfMeasurement")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let state_raw_str = format!("{st_raw}{unit}");
        let state_display = encode_text(&state_raw_str);
        let state_cls = if st_raw.eq_ignore_ascii_case("on") {
            "badge up"
        } else if st_raw.eq_ignore_ascii_case("off") {
            "badge muted"
        } else {
            "badge"
        };
        html.push_str(&format!(
            r#"<div class="smart-entity-tile">
  <span class="smart-entity-name">{name}</span>
  <span class="{state_cls}">{state_display}</span>
</div>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_notifications(data: &Value) -> String {
    let items = data
        .get("notifications")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return r#"<div class="widget-empty">No recent notifications</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for n in items.iter().take(6) {
        let title = encode_text(n.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        html.push_str(&format!(r#"<li>{title}</li>"#));
    }
    html.push_str("</ul>");
    html
}

fn render_indexers(data: &Value) -> String {
    let items = data
        .get("indexers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return r#"<div class="widget-empty">No indexers</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for idx in items.iter().take(8) {
        let name = encode_text(idx.get("name").and_then(|v| v.as_str()).unwrap_or(""));
        let st = encode_text(idx.get("status").and_then(|v| v.as_str()).unwrap_or("ok"));
        html.push_str(&format!(
            r#"<li><span>{name}</span> <span class="badge">{st}</span></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_ups(data: &Value) -> String {
    let Some(first) = data
        .get("ups")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
    else {
        return r#"<div class="widget-empty">No UPS telemetry</div>"#.to_string();
    };
    let name = encode_text(first.get("name").and_then(|v| v.as_str()).unwrap_or("UPS"));
    let charge = first
        .get("batteryChargePercent")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let load = first
        .get("loadPercent")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">{name} Battery</span><strong>{charge:.0}%</strong></div>
  <div class="stat-pill"><span class="label">Load</span><strong>{load:.0}%</strong></div>
</div>"#
    )
}

fn render_stats(data: &Value) -> String {
    let metrics = data
        .get("stats")
        .and_then(|s| s.get("metrics"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if metrics.is_empty() {
        return r#"<div class="widget-empty">No stats available</div>"#.to_string();
    }
    let mut html = String::from(r#"<div class="stats-grid">"#);
    for m in metrics.iter().take(6) {
        let label = encode_text(m.get("label").and_then(|v| v.as_str()).unwrap_or(""));
        let value = encode_text(m.get("value").and_then(|v| v.as_str()).unwrap_or(""));
        html.push_str(&format!(
            r#"<div class="stat-pill"><span class="label">{label}</span><strong>{value}</strong></div>"#
        ));
    }
    html.push_str("</div>");
    html
}

fn render_custom_api(data: &Value) -> String {
    if let Some(rendered) = data.get("renderedHtml").and_then(|v| v.as_str()) {
        // Template placeholders were already HTML-escaped by `render_template`.
        return format!(r#"<div class="custom-widget-content">{rendered}</div>"#);
    }
    r#"<div class="widget-empty">Configure a Custom API URL or Custom Widget v2 definition</div>"#
        .to_string()
}

fn render_assistant(options: &Value) -> String {
    let prompt = encode_text(
        options
            .get("placeholder")
            .and_then(|v| v.as_str())
            .unwrap_or("Ask Homarr Assistant..."),
    );
    format!(
        r#"<div class="assistant-cta">
  <p class="metric-sub">{prompt}</p>
  <a class="btn btn-sm" href="/manage/assistant">Open Assistant</a>
</div>"#
    )
}

fn render_stats_with_extras(kind: WidgetKind, data: &Value) -> String {
    let mut html = render_stats(data);
    if kind == WidgetKind::BeszelAlerts {
        if let Some(alerts) = data.get("alerts").and_then(|v| v.as_array()) {
            if !alerts.is_empty() {
                html.push_str(r#"<ul class="compact-list" style="margin-top:8px">"#);
                for alert in alerts.iter().take(4) {
                    let name = encode_text(
                        alert
                            .get("name")
                            .or_else(|| alert.get("id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Alert"),
                    );
                    let level_str = alert
                        .get("value")
                        .and_then(|v| v.as_f64())
                        .map(|n| format!("{n:.0}"))
                        .unwrap_or_else(|| "active".to_string());
                    let level = encode_text(&level_str);
                    html.push_str(&format!(
                        r#"<li><span>{name}</span> <span class="badge">{level}</span></li>"#
                    ));
                }
                html.push_str("</ul>");
            }
        }
    }
    html
}

fn render_error_box(data: &Value) -> Option<String> {
    data.get("error")
        .and_then(|v| v.as_str())
        .map(|err| format!(r#"<div class="widget-empty">{}</div>"#, encode_text(err)))
}

fn render_media_releases(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let releases = data
        .get("releases")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if releases.is_empty() {
        return r#"<div class="widget-empty">No recent media releases</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for rel in releases.iter().take(6) {
        let title = encode_text(
            rel.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled"),
        );
        let sub = rel
            .get("subtitle")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| format!(r#" <span class="muted">{}</span>"#, encode_text(s)))
            .unwrap_or_default();
        let href = safe_url(rel.get("href").and_then(|v| v.as_str()).unwrap_or("#"));
        let date_str = rel
            .get("releaseDate")
            .and_then(|v| v.as_str())
            .map(|s| s.chars().take(10).collect::<String>())
            .unwrap_or_default();
        let date = encode_text(&date_str);
        let title_el = if href == "#" {
            format!(r#"<span>{title}{sub}</span>"#)
        } else {
            format!(
                r#"<a href="{}" target="_blank" rel="noopener noreferrer">{title}{sub}</a>"#,
                encode_attr(href)
            )
        };
        html.push_str(&format!(
            r#"<li>{title_el} <span class="badge">{date}</span></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_media_missing(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return r#"<div class="widget-empty">No missing media</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for item in items.iter().take(8) {
        let title = encode_text(item.get("title").and_then(|v| v.as_str()).unwrap_or(""));
        let sub = encode_text(
            item.get("subtitle")
                .and_then(|v| v.as_str())
                .unwrap_or("Missing"),
        );
        html.push_str(&format!(
            r#"<li><span>{title}</span> <span class="badge">{sub}</span></li>"#
        ));
    }
    html.push_str("</ul>");
    html
}

fn render_media_transcoding(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let Some(summary) = data.get("summary") else {
        return r#"<div class="widget-empty">Transcoding stats unavailable</div>"#.to_string();
    };
    let files = summary
        .get("totalFiles")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let queue = summary
        .get("queueCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let workers = summary
        .get("activeWorkers")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let saved = summary
        .get("spaceSavedBytes")
        .and_then(|v| v.as_u64())
        .map(format_si_bytes)
        .unwrap_or_else(|| "0 B".to_string());
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">Files</span><strong>{files}</strong></div>
  <div class="stat-pill"><span class="label">Queue</span><strong>{queue}</strong></div>
  <div class="stat-pill"><span class="label">Workers</span><strong>{workers}</strong></div>
  <div class="stat-pill"><span class="label">Saved</span><strong>{}</strong></div>
</div>"#,
        encode_text(&saved)
    )
}

fn render_firewall(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let cpu = data
        .get("cpu")
        .and_then(|c| c.get("total"))
        .and_then(|v| v.as_f64())
        .map(|v| format!("{v:.1}%"))
        .unwrap_or_else(|| "—".to_string());
    let mem = data
        .get("memory")
        .and_then(|m| m.get("percent"))
        .and_then(|v| v.as_f64())
        .map(|v| format!("{v:.1}%"))
        .unwrap_or_else(|| "—".to_string());
    let ver = data
        .get("version")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("OPNsense");
    let if_count = data
        .get("interfaces")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|entry| entry.get("data"))
        .and_then(|d| d.as_array())
        .map(|d| d.len())
        .unwrap_or(0);
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">CPU</span><strong>{}</strong></div>
  <div class="stat-pill"><span class="label">Memory</span><strong>{}</strong></div>
  <div class="stat-pill"><span class="label">Interfaces</span><strong>{if_count}</strong></div>
  <div class="stat-pill"><span class="label">Version</span><strong>{}</strong></div>
</div>"#,
        encode_text(&cpu),
        encode_text(&mem),
        encode_text(ver),
    )
}

fn render_vpn(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let Some(vpn) = data.get("vpn") else {
        return r#"<div class="widget-empty">VPN status unavailable</div>"#.to_string();
    };
    let status = encode_text(
        vpn.get("vpnStatus")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
    );
    let ip = encode_text(vpn.get("publicIp").and_then(|v| v.as_str()).unwrap_or("—"));
    let country = encode_text(vpn.get("country").and_then(|v| v.as_str()).unwrap_or("—"));
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">Status</span><strong>{status}</strong></div>
  <div class="stat-pill"><span class="label">Public IP</span><strong>{ip}</strong></div>
  <div class="stat-pill"><span class="label">Country</span><strong>{country}</strong></div>
</div>"#
    )
}

fn render_traefik(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let Some(t) = data.get("traefik") else {
        return r#"<div class="widget-empty">Traefik overview unavailable</div>"#.to_string();
    };
    let sum_field = |group: &str| -> u64 {
        ["http", "tcp", "udp"]
            .iter()
            .filter_map(|p| {
                t.get(p)
                    .and_then(|x| x.get(group))
                    .and_then(|g| g.get("total"))
                    .and_then(|v| v.as_u64())
            })
            .sum()
    };
    let routers = sum_field("routers");
    let services = sum_field("services");
    let middlewares = sum_field("middlewares");
    let version = encode_text(
        t.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("Traefik"),
    );
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">Routers</span><strong>{routers}</strong></div>
  <div class="stat-pill"><span class="label">Services</span><strong>{services}</strong></div>
  <div class="stat-pill"><span class="label">Middleware</span><strong>{middlewares}</strong></div>
  <div class="stat-pill"><span class="label">Version</span><strong>{version}</strong></div>
</div>"#
    )
}

fn render_network_controller(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    let Some(net) = data.get("network") else {
        return r#"<div class="widget-empty">Network controller unavailable</div>"#.to_string();
    };
    let wifi = net
        .get("wifi")
        .map(|w| {
            w.get("users").and_then(|v| v.as_u64()).unwrap_or(0)
                + w.get("guests").and_then(|v| v.as_u64()).unwrap_or(0)
        })
        .unwrap_or(0);
    let lan = net
        .get("lan")
        .map(|l| {
            l.get("users").and_then(|v| v.as_u64()).unwrap_or(0)
                + l.get("guests").and_then(|v| v.as_u64()).unwrap_or(0)
        })
        .unwrap_or(0);
    let latency = net
        .get("www")
        .and_then(|w| w.get("latency"))
        .and_then(|v| v.as_f64())
        .map(|l| format!("{l:.1} ms"))
        .unwrap_or_else(|| "—".to_string());
    let wan = encode_text(
        net.get("wanStatus")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
    );
    format!(
        r#"<div class="stats-grid">
  <div class="stat-pill"><span class="label">WAN</span><strong>{wan}</strong></div>
  <div class="stat-pill"><span class="label">Wi-Fi</span><strong>{wifi}</strong></div>
  <div class="stat-pill"><span class="label">LAN</span><strong>{lan}</strong></div>
  <div class="stat-pill"><span class="label">Latency</span><strong>{}</strong></div>
</div>"#,
        encode_text(&latency)
    )
}

fn render_anchor_note(data: &Value) -> String {
    if let Some(err_html) = render_error_box(data) {
        return err_html;
    }
    if let Some(note) = data.get("note") {
        let title = encode_text(note.get("title").and_then(|v| v.as_str()).unwrap_or("Note"));
        let content = encode_text(note.get("content").and_then(|v| v.as_str()).unwrap_or(""));
        return format!(
            r#"<div class="notebook-body"><strong>{title}</strong><p style="margin-top:6px;white-space:pre-wrap">{content}</p></div>"#
        );
    }
    let notes = data
        .get("notes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if notes.is_empty() {
        return r#"<div class="widget-empty">No notes found</div>"#.to_string();
    }
    let mut html = String::from(r#"<ul class="compact-list">"#);
    for note in notes.iter().take(6) {
        let title = encode_text(
            note.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled"),
        );
        let pinned = if note
            .get("isPinned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            r#"<span class="badge">pinned</span>"#
        } else {
            ""
        };
        html.push_str(&format!(r#"<li><span>{title}</span> {pinned}</li>"#));
    }
    html.push_str("</ul>");
    html
}

fn render_generic(data: &Value) -> String {
    if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
        return format!(r#"<div class="widget-empty">{}</div>"#, encode_text(err));
    }
    let raw = data.get("data").unwrap_or(data);
    let summary = raw.to_string();
    let clipped = if summary.chars().count() > 280 {
        format!("{}...", summary.chars().take(280).collect::<String>())
    } else {
        summary
    };
    format!(
        r#"<pre class="generic-json">{}</pre>"#,
        encode_text(&clipped)
    )
}

#[cfg(test)]
mod notebook_sanitizer_tests {
    use super::sanitize_notebook_html as clean;

    #[test]
    fn removes_scripts_and_event_attributes() {
        assert_eq!(clean("<p>a<script>alert(1)</script>b</p>"), "<p>ab</p>");
        assert_eq!(clean("<SCRIPT>x</SCRIPT >ok"), "ok");
        assert_eq!(clean(r#"<img src=x onerror="alert(1)">t"#), "t");
        assert_eq!(
            clean(r#"<p onclick="x()" style="color:red">t</p>"#),
            "<p>t</p>"
        );
        assert_eq!(clean("<style>p{}</style><svg onload=x><p>a</p></svg>"), "");
        assert_eq!(clean("a < b > c <!-- <script> -->"), "a &lt; b &gt; c ");
        assert_eq!(clean(r#"<p title="a>b">t</p>"#), "<p>t</p>");
        assert_eq!(clean("<p>unterminated <b"), "<p>unterminated </p>");
    }

    #[test]
    fn filters_link_targets() {
        assert_eq!(clean(r#"<a href="javascript:alert(1)">x</a>"#), "<a>x</a>");
        assert_eq!(clean(r#"<a href=" JaVaScRiPt:alert(1)">x</a>"#), "<a>x</a>");
        assert_eq!(clean(r#"<a href="jav&#x61;script:x">x</a>"#), "<a>x</a>");
        assert_eq!(
            clean(r#"<a onclick="x" href='https://e.com/?a=1&amp;b="2"'>x</a>"#),
            r#"<a href="https://e.com/?a=1&amp;b=&quot;2&quot;" target="_blank" rel="noopener noreferrer">x</a>"#
        );
        assert!(clean(r#"<a href="mailto:a@b.c">m</a>"#).contains(r#"href="mailto:a@b.c""#));
    }

    #[test]
    fn keeps_formatting() {
        let html =
            "<h2>T</h2><p><strong>b</strong> <em>i</em> <u>u</u> <s>s</s> <code>c</code></p>\
                    <ul><li>x</li></ul><ol><li>y</li></ol><blockquote>q</blockquote><pre>p</pre>\
                    <hr><br><table><tbody><tr><td>c</td></tr></tbody></table>";
        assert_eq!(clean(html), html);
        assert_eq!(
            clean(r#"<p style="text-align:center"><span style="color:red">r</span></p>"#),
            "<p>r</p>"
        );
        // Unbalanced markup is closed.
        assert_eq!(clean("<ul><li>a</ul>b</p>"), "<ul><li>a</li></ul>b");
        assert_eq!(clean("<p><em>a"), "<p><em>a</em></p>");
    }
}
