use homarr_auth::AuthSession;
use homarr_db::models::BoardRecord;
use homarr_widgets::safe_url;
use html_escape::encode_text;
use serde_json::Value;

use crate::i18n::I18nCatalog;
use crate::icons;

pub struct PageContext<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub session: Option<&'a AuthSession>,
    pub demo_mode: bool,
    pub demo_read_only: bool,
    pub boards: &'a [BoardRecord],
    pub active_board_name: Option<&'a str>,
    pub locale: &'a str,
    pub i18n: &'a I18nCatalog,
    pub custom_css: Option<&'a str>,
    pub primary_color: Option<&'a str>,
    pub secondary_color: Option<&'a str>,
    pub background_image_url: Option<&'a str>,
    pub background_image_attachment: Option<&'a str>,
    pub background_image_repeat: Option<&'a str>,
    pub background_image_size: Option<&'a str>,
    pub opacity: Option<i32>,
    pub item_radius: Option<&'a str>,
    pub logo_image_url: Option<&'a str>,
    pub favicon_image_url: Option<&'a str>,
    pub spotlight_items: &'a [Value],
    pub can_edit_board: bool,
    pub is_board_page: bool,
    pub hide_header: bool,
}

impl<'a> PageContext<'a> {
    pub fn minimal(title: &'a str, body: &'a str, locale: &'a str, i18n: &'a I18nCatalog) -> Self {
        Self {
            title,
            body,
            session: None,
            demo_mode: false,
            demo_read_only: false,
            boards: &[],
            active_board_name: None,
            locale,
            i18n,
            custom_css: None,
            primary_color: None,
            secondary_color: None,
            background_image_url: None,
            background_image_attachment: None,
            background_image_repeat: None,
            background_image_size: None,
            opacity: None,
            item_radius: None,
            logo_image_url: None,
            favicon_image_url: None,
            spotlight_items: &[],
            can_edit_board: false,
            is_board_page: false,
            hide_header: false,
        }
    }
}

fn sanitize_css(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("</style")
        || lower.contains("<script")
        || lower.contains("javascript:")
        || lower.contains("expression(")
    {
        return String::new();
    }
    raw.to_string()
}

fn sanitize_hex_color(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.starts_with('#')
        && (s.len() == 4 || s.len() == 7)
        && s[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        Some(s.to_string())
    } else {
        None
    }
}

fn sanitize_css_url(raw: Option<&str>) -> Option<String> {
    let candidate = raw?.trim();
    if candidate.is_empty() {
        return None;
    }
    let safe = safe_url(candidate);
    if safe == "#"
        || safe.contains('\'')
        || safe.contains('"')
        || safe.contains('(')
        || safe.contains(')')
        || safe.contains('\\')
    {
        return None;
    }
    Some(safe.to_string())
}

fn map_item_radius(raw: Option<&str>) -> Option<&'static str> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "xs" => Some("4px"),
        "sm" => Some("6px"),
        "md" => Some("8px"),
        "lg" => Some("12px"),
        "xl" => Some("16px"),
        _ => None,
    }
}

fn sanitize_bg_attachment(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("fixed").trim().to_ascii_lowercase().as_str() {
        "scroll" => "scroll",
        "local" => "local",
        _ => "fixed",
    }
}

fn sanitize_bg_repeat(raw: Option<&str>) -> &'static str {
    match raw
        .unwrap_or("no-repeat")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "repeat" => "repeat",
        "repeat-x" => "repeat-x",
        "repeat-y" => "repeat-y",
        _ => "no-repeat",
    }
}

fn sanitize_bg_size(raw: Option<&str>) -> &'static str {
    match raw.unwrap_or("cover").trim().to_ascii_lowercase().as_str() {
        "contain" => "contain",
        "auto" => "auto",
        _ => "cover",
    }
}

fn urlencoding(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn render_page(ctx: PageContext<'_>) -> String {
    let theme = ctx
        .session
        .map(|s| {
            if s.color_scheme == "light" {
                "light"
            } else {
                "dark"
            }
        })
        .unwrap_or("dark");

    let custom_style = {
        let mut s = String::new();
        if let Some(p) = sanitize_hex_color(ctx.primary_color) {
            s.push_str(&format!(
                ":root{{--primary:{p};--primary-hover:color-mix(in srgb,{p} 82%,#000);}}"
            ));
        }
        if let Some(sec) = sanitize_hex_color(ctx.secondary_color) {
            s.push_str(&format!(":root{{--secondary:{sec};--blue:{sec};}}"));
        }
        if let Some(op) = ctx.opacity {
            let frac = (op.clamp(0, 100) as f32) / 100.0;
            s.push_str(&format!(":root{{--opacity:{frac:.2};}}"));
        }
        if let Some(rad) = map_item_radius(ctx.item_radius) {
            s.push_str(&format!(":root{{--item-radius:{rad};}}"));
        }
        if let Some(css) = ctx.custom_css {
            s.push_str(&sanitize_css(css));
        }
        if s.is_empty() {
            String::new()
        } else {
            format!("<style>{s}</style>")
        }
    };

    let favicon_href =
        sanitize_css_url(ctx.favicon_image_url).unwrap_or_else(|| "/favicon.ico".to_string());
    let logo_src =
        sanitize_css_url(ctx.logo_image_url).unwrap_or_else(|| "/logo/homarr.svg".to_string());

    let background_layer = match sanitize_css_url(ctx.background_image_url) {
        Some(bg_url) => {
            let attach = sanitize_bg_attachment(ctx.background_image_attachment);
            let repeat = sanitize_bg_repeat(ctx.background_image_repeat);
            let size = sanitize_bg_size(ctx.background_image_size);
            format!(
                r#"<div class="board-bg-layer" style="background-image:url('{}');background-attachment:{attach};background-repeat:{repeat};background-size:{size}" aria-hidden="true"></div>"#,
                encode_text(&bg_url)
            )
        }
        None => String::new(),
    };

    let mut locale_select_opts = String::new();
    for code in ctx.i18n.codes() {
        let selected = if code == ctx.locale { " selected" } else { "" };
        locale_select_opts.push_str(&format!(
            r#"<option value="{}"{selected}>{}</option>"#,
            encode_text(code),
            encode_text(code)
        ));
    }

    let locale_picker_inline = if ctx.i18n.codes().is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="locale-chip" title="Language">{}<select class="locale-select" aria-label="Language" onchange="window.setLocaleCookie(this.value)">{locale_select_opts}</select></div>"#,
            icons::GLOBE
        )
    };

    let board_switcher = if ctx.boards.is_empty() {
        String::new()
    } else {
        let active_label = ctx
            .active_board_name
            .or_else(|| ctx.boards.first().map(|b| b.name.as_str()))
            .unwrap_or("Boards");
        let mut items_html = String::new();
        for b in ctx.boards {
            let is_active = ctx.active_board_name == Some(b.name.as_str());
            let active_cls = if is_active { " is-active" } else { "" };
            let check = if is_active { icons::CHECK } else { "" };
            items_html.push_str(&format!(
                r#"<a class="dropdown-item{active_cls}" href="/boards/{enc_url}"><span>{}</span><span class="dropdown-check">{check}</span></a>"#,
                encode_text(&b.name),
                enc_url = urlencoding(&b.name),
            ));
        }
        let manage_boards = if ctx.session.is_some_and(AuthSession::is_admin) {
            format!(
                r#"<div class="dropdown-divider"></div><a class="dropdown-item" href="/manage/boards">{}<span>Manage Boards</span></a>"#,
                icons::PLUS
            )
        } else {
            String::new()
        };
        format!(
            r#"<div class="dropdown">
  <button type="button" class="board-switcher-btn" onclick="window.toggleDropdown(this)" aria-expanded="false" aria-label="Switch Board">
    {dash_icon}
    <span class="board-switcher-label">{active_label}</span>
    {chev}
  </button>
  <div class="dropdown-menu dropdown-menu-left">
    <div class="dropdown-header">Boards</div>
    {items_html}
    {manage_boards}
  </div>
</div>"#,
            dash_icon = icons::LAYOUT_DASHBOARD,
            active_label = encode_text(active_label),
            chev = icons::CHEVRON_DOWN,
        )
    };

    let demo_badge = if ctx.demo_mode {
        if ctx.demo_read_only {
            r#"<span class="demo-pill" title="Demo Read-Only">Demo</span>"#
        } else {
            r#"<span class="demo-pill" title="Demo Mode">Demo</span>"#
        }
    } else {
        ""
    };

    let edit_btn = if ctx.can_edit_board && ctx.is_board_page {
        format!(
            r#"<button type="button" id="edit-mode-btn" class="icon-btn" onclick="window.toggleEditMode()" title="Edit Board" aria-label="Edit Board">{}</button>"#,
            icons::PENCIL
        )
    } else {
        String::new()
    };

    let board_settings_btn = if ctx.can_edit_board && ctx.is_board_page {
        if let Some(active) = ctx.active_board_name {
            format!(
                r#"<a class="icon-btn" href="/boards/{}/settings" title="Board Settings" aria-label="Board Settings">{}</a>"#,
                urlencoding(active),
                icons::SETTINGS
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let manage_icon_btn = if ctx.session.is_some_and(AuthSession::is_admin) {
        format!(
            r#"<a class="icon-btn" href="/manage" title="Manage Homarr" aria-label="Manage Homarr">{}</a>"#,
            icons::SHIELD
        )
    } else {
        String::new()
    };

    let auth_controls = match ctx.session {
        Some(sess) => {
            let raw_name = sess.username.as_deref().unwrap_or("User");
            let uname = encode_text(raw_name);
            let initial = raw_name.chars().next().unwrap_or('U').to_ascii_uppercase();
            let manage_menu_item = if sess.is_admin() {
                format!(
                    r#"<a class="dropdown-item" href="/manage">{}<span>Manage Workspace</span></a>"#,
                    icons::SHIELD
                )
            } else {
                String::new()
            };
            let settings_menu_item = if ctx.can_edit_board {
                if let Some(active) = ctx.active_board_name {
                    format!(
                        r#"<a class="dropdown-item" href="/boards/{}/settings">{}<span>Board Settings</span></a>"#,
                        urlencoding(active),
                        icons::SETTINGS
                    )
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let locale_menu_item = if ctx.i18n.codes().is_empty() {
                String::new()
            } else {
                format!(
                    r#"<div class="dropdown-item dropdown-item-static">{}<span style="flex:1">Language</span><select class="locale-select" aria-label="Language" onchange="window.setLocaleCookie(this.value)">{locale_select_opts}</select></div>"#,
                    icons::GLOBE
                )
            };
            format!(
                r#"<div class="dropdown">
  <button type="button" class="user-trigger" onclick="window.toggleDropdown(this)" aria-expanded="false" aria-label="User menu">
    <span class="avatar-circle">{initial}</span>
    <span class="user-trigger-name">{uname}</span>
    {chev}
  </button>
  <div class="dropdown-menu dropdown-menu-right">
    <div class="dropdown-header">Signed in as <strong>{uname}</strong></div>
    {manage_menu_item}
    {settings_menu_item}
    <a class="dropdown-item" href="/reference/api">{book}<span>API Reference</span></a>
    {locale_menu_item}
    <div class="dropdown-divider"></div>
    <button type="button" class="dropdown-item dropdown-item-danger" data-api-action="/api/auth/logout" data-method="POST">{logout}<span>Sign Out</span></button>
  </div>
</div>"#,
                chev = icons::CHEVRON_DOWN,
                book = icons::BOOK,
                logout = icons::LOGOUT,
            )
        }
        None => {
            format!(
                r#"{locale_picker_inline}<a class="btn btn-primary btn-sm" href="/auth/login">Sign In</a>"#
            )
        }
    };

    let header_html = if ctx.hide_header {
        String::new()
    } else {
        format!(
            r#"<header class="topbar">
  <div class="header-content">
    <div class="header-zone header-zone-left">
      <a class="brand" href="/">
        <img class="brand-logo" src="{logo_src}" alt="Homarr" />
        <span class="brand-name">Homarr</span>
      </a>
      {demo_badge}
      {board_switcher}
    </div>
    <div class="header-zone header-zone-center">
      <button type="button" class="spotlight-trigger" onclick="window.openSpotlight()" aria-label="Search boards and apps">
        <span class="spotlight-trigger-inner">{search_icon}<span>Search boards, apps, or web...</span></span>
        <span class="kbd">Ctrl K</span>
      </button>
    </div>
    <div class="header-zone header-zone-right">
      {edit_btn}
      {board_settings_btn}
      <button type="button" class="icon-btn" onclick="window.toggleTheme()" title="Toggle color scheme" aria-label="Toggle color scheme">{theme_icon}</button>
      {manage_icon_btn}
      {auth_controls}
    </div>
  </div>
</header>"#,
            logo_src = encode_text(&logo_src),
            search_icon = icons::SEARCH,
            theme_icon = icons::SUN_MOON,
        )
    };

    let main_cls = if ctx.hide_header {
        "main-container main-auth"
    } else if ctx.is_board_page {
        "main-container board-canvas"
    } else {
        "main-container"
    };

    let body_cls = if ctx.background_image_url.is_some() {
        "has-board-bg"
    } else {
        ""
    };

    let spotlight_script_json = serde_json::to_string(ctx.spotlight_items)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/");

    format!(
        r#"<!doctype html>
<html lang="{lang}" data-theme="{theme}">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title} — Homarr</title>
  <link rel="icon" href="{favicon}" />
  <link rel="stylesheet" href="/static/homarr.css" />
  {custom_style}
</head>
<body class="{body_cls}">
  {background_layer}
  {header_html}

  <main class="{main_cls}">
    {body}
  </main>

  <div id="spotlight-modal" class="modal-backdrop" onclick="if(event.target===this)window.closeSpotlight()">
    <div class="modal-card spotlight-card">
      <div class="spotlight-input-row">
        {search_icon}
        <input id="spotlight-input" type="search" placeholder="Search boards, apps, integrations, or web..." autocomplete="off" />
        <button type="button" class="icon-btn icon-btn-sm" onclick="window.closeSpotlight()" aria-label="Close search">{close_icon}</button>
      </div>
      <div id="spotlight-results" class="spotlight-results"></div>
      <div class="spotlight-footer">
        <span><kbd class="kbd">↑</kbd> <kbd class="kbd">↓</kbd> Navigate</span>
        <span><kbd class="kbd">↵</kbd> Open</span>
        <span><kbd class="kbd">Esc</kbd> Close</span>
      </div>
    </div>
  </div>

  <div id="toast-stack" class="toast-stack"></div>
  <script>window.__HOMARR_SPOTLIGHT__ = {spotlight_script_json};</script>
  <script src="/static/homarr.js" defer></script>
</body>
</html>"#,
        lang = encode_text(ctx.locale),
        title = encode_text(ctx.title),
        favicon = encode_text(&favicon_href),
        body = ctx.body,
        search_icon = icons::SEARCH,
        close_icon = icons::X,
    )
}
