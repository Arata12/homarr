pub mod registry;
pub mod render;
pub mod resolver;

pub use registry::{all_widget_metadata, get_widget_metadata, WidgetMetadata};
pub use render::{render_widget_html, safe_url};
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
    }
}
