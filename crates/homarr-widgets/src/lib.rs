pub mod registry;
pub mod render;
pub mod resolver;

pub use registry::{
    all_widget_metadata, get_widget_metadata, sanitize_widget_options, widget_option_fields,
    OptionChoice, OptionField, WidgetMetadata,
};
pub use render::{render_widget_html, render_widget_html_with_origin, safe_url};
pub use resolver::WidgetResolver;

#[cfg(test)]
mod tests {
    use super::*;
    use homarr_definitions::WidgetKind;
    use serde_json::json;

    #[test]
    fn registry_covers_all_67_widget_kinds() {
        let all = all_widget_metadata();
        assert_eq!(all.len(), WidgetKind::ALL.len());
        assert_eq!(all.len(), 67);
        for kind in WidgetKind::ALL {
            assert!(all.iter().any(|m| m.kind == kind.as_str()));
        }
    }

    #[test]
    fn render_escapes_xss_and_blocks_unsafe_urls() {
        let html = render_widget_html(
            WidgetKind::Notebook,
            "item-1",
            &json!({ "content": "<script>alert('xss')</script>" }),
            &json!({}),
        );
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));

        let bad_iframe = render_widget_html(
            WidgetKind::Iframe,
            "item-2",
            &json!({ "embedUrl": "javascript:alert(1)" }),
            &json!({}),
        );
        assert!(!bad_iframe.contains("javascript:"));
        assert!(bad_iframe.contains("Invalid embed URL"));

        let app_html = render_widget_html(
            WidgetKind::App,
            "item-3",
            &json!({}),
            &json!({
                "app": {
                    "name": "Sonarr<img src=x onerror=alert(1)>",
                    "description": "TV",
                    "iconUrl": "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/sonarr.svg",
                    "href": "https://sonarr.local"
                }
            }),
        );
        assert!(!app_html.contains("<img src=x"));
        assert!(app_html.contains("&lt;img src=x"));

        // Attribute quote breakout attempts must be escaped in double-quoted attributes
        let countdown_html = render_widget_html(
            WidgetKind::Countdown,
            "item-4",
            &json!({ "title": "Launch", "date": "\" onclick=\"alert(1)" }),
            &json!({}),
        );
        assert!(countdown_html.contains("data-target-date=\"&quot; onclick=&quot;alert(1)\""));

        let app_attr_xss = render_widget_html(
            WidgetKind::App,
            "item-5",
            &json!({}),
            &json!({
                "app": {
                    "name": "App\" onmouseover=\"alert(1)",
                    "description": "Desc\" onfocus=\"alert(2)",
                    "iconUrl": "https://cdn.example/icon.png",
                    "href": "https://app.local"
                }
            }),
        );
        assert!(app_attr_xss.contains("&quot; onmouseover=&quot;alert(1)"));
        assert!(app_attr_xss.contains("&quot; onfocus=&quot;alert(2)"));

        // Protocol-relative URLs and quote-containing URLs must be rejected by safe_url
        assert_eq!(safe_url("//evil.example/a"), "#");
        assert_eq!(safe_url("https://good.example/\"onclick=1"), "#");

        // Same-origin absolute URLs in iframe must drop allow-same-origin
        let same_origin_iframe = render_widget_html_with_origin(
            WidgetKind::Iframe,
            "item-6",
            &json!({ "embedUrl": "https://homarr.example/api/medias/1" }),
            &json!({}),
            Some("https://homarr.example"),
        );
        assert!(!same_origin_iframe.contains("allow-same-origin"));
    }

    #[test]
    fn sanitize_preserves_bookmarks_and_stats_object_arrays() {
        let bm = sanitize_widget_options(
            WidgetKind::Bookmarks,
            &json!({
                "items": [{ "name": "Docs", "href": "https://homarr.dev" }],
                "layout": "grid"
            }),
        );
        assert_eq!(bm["items"][0]["name"], "Docs");
        assert_eq!(bm["items"][0]["href"], "https://homarr.dev");

        let stats = sanitize_widget_options(
            WidgetKind::Stats,
            &json!({
                "metrics": [{ "key": "cpu", "label": "CPU" }]
            }),
        );
        assert_eq!(stats["metrics"][0]["key"], "cpu");
    }

    #[test]
    fn renders_media_releases_and_utf8_generic_safely() {
        let rel_html = render_widget_html(
            WidgetKind::MediaReleases,
            "item-7",
            &json!({}),
            &json!({
                "releases": [{
                    "title": "Movie<script>1</script>",
                    "subtitle": "S01E01",
                    "href": "javascript:alert(1)",
                    "releaseDate": "2026-05-01T00:00:00Z"
                }]
            }),
        );
        assert!(!rel_html.contains("<script>"));
        assert!(!rel_html.contains("javascript:"));
        assert!(rel_html.contains("2026-05-01"));

        let multi_byte = "é".repeat(300);
        let generic_html = render_widget_html(
            WidgetKind::Coolify,
            "item-8",
            &json!({}),
            &json!({ "text": multi_byte }),
        );
        assert!(generic_html.contains("..."));
    }

    #[test]
    fn renders_notebook_markdown_safely() {
        let md = "# Homelab Notes\n## Tasks\n- [x] Done **task**\n- [ ] Check `status`\nPlain <img src=x onerror=alert(1)>";
        let html = render_widget_html(
            WidgetKind::Notebook,
            "item-md",
            &json!({ "content": md }),
            &json!({}),
        );
        assert!(html.contains("<h4>Homelab Notes</h4>"));
        assert!(html.contains("<h5>Tasks</h5>"));
        assert!(html.contains("<strong>task</strong>"));
        assert!(html.contains("<code>status</code>"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("&lt;img src=x"));
    }
}
