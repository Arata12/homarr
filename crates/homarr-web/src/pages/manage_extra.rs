use axum::{
    extract::State,
    http::HeaderMap,
    response::{Html, IntoResponse},
};
use homarr_api::openapi::openapi_spec;
use homarr_auth::{AuthSession, OptionalAuth};
use homarr_db::repos::{boards, search_engines, settings};
use homarr_definitions::{
    integration::{all_integration_defs, IntegrationSecretKind},
    settings::parse_branding_settings,
};
use homarr_widgets::safe_url;
use html_escape::{encode_double_quoted_attribute as encode_attr, encode_text};
use serde_json::{json, Map, Value};

use super::manage::{empty_table_row, render_manage_shell, require_admin_or_redirect};
use crate::layout::{build_spotlight_items, render_page, PageContext, ServerUiSettings};
use crate::WebState;

fn secret_kind_label(kind: IntegrationSecretKind) -> &'static str {
    match kind {
        IntegrationSecretKind::ApiKey => "API Key",
        IntegrationSecretKind::Username => "Username",
        IntegrationSecretKind::Password => "Password",
        IntegrationSecretKind::TokenId => "Token ID",
        IntegrationSecretKind::Realm => "Realm",
        IntegrationSecretKind::PersonalAccessToken => "Personal Access Token",
        IntegrationSecretKind::Topic => "Topic",
        IntegrationSecretKind::OpnsenseApiKey => "OPNsense API Key",
        IntegrationSecretKind::OpnsenseApiSecret => "OPNsense API Secret",
        IntegrationSecretKind::PatchmonApiKey => "PatchMon API Key",
        IntegrationSecretKind::PatchmonApiSecret => "PatchMon API Secret",
        IntegrationSecretKind::Url => "Secondary URL",
        IntegrationSecretKind::PrivateKey => "Private Key",
        IntegrationSecretKind::GithubAppId => "GitHub App ID",
        IntegrationSecretKind::GithubInstallationId => "GitHub Installation ID",
        IntegrationSecretKind::Slug => "Slug",
        IntegrationSecretKind::WazuhIndexerUrl => "Wazuh Indexer URL",
        IntegrationSecretKind::WazuhIndexerUsername => "Wazuh Indexer Username",
        IntegrationSecretKind::WazuhIndexerPassword => "Wazuh Indexer Password",
        IntegrationSecretKind::TotpSecret => "TOTP Secret",
    }
}

/// Serializes all 92 integration definitions for the dynamic credential form in `/manage/integrations`.
pub(super) fn integration_defs_map_json() -> Value {
    let mut map = Map::new();
    for def in all_integration_defs() {
        let mut seen = Vec::new();
        let mut push_secret = |sk: IntegrationSecretKind, optional: bool| {
            let key = sk.as_str();
            if !seen.iter().any(|(k, _)| k == key) {
                seen.push((key.to_string(), (sk, optional)));
            }
        };
        for group in def.secret_kinds {
            for sk in *group {
                push_secret(*sk, false);
            }
        }
        for sk in def.optional_secret_kinds {
            push_secret(*sk, true);
        }
        let secrets: Vec<Value> = seen
            .into_iter()
            .map(|(k, (sk, opt))| {
                let meta = sk.metadata();
                json!({
                    "kind": k,
                    "label": secret_kind_label(sk),
                    "optional": opt,
                    "isPublic": meta.is_public,
                    "multiline": meta.multiline,
                })
            })
            .collect();
        let default_url = def.default_url.map(str::to_string).unwrap_or_else(|| {
            if let Some(p) = def.default_port {
                format!("http://127.0.0.1:{p}")
            } else {
                "https://service.local".to_string()
            }
        });
        map.insert(
            def.kind_str.to_string(),
            json!({
                "name": def.name,
                "defaultUrl": default_url,
                "secrets": secrets,
            }),
        );
    }
    Value::Object(map)
}

fn check_row(name: &str, label: &str, checked: bool, hint: &str) -> String {
    let c = if checked { "checked" } else { "" };
    let hint_html = if hint.is_empty() {
        String::new()
    } else {
        format!(r#"<span class="muted" style="font-size:.76rem">{hint}</span>"#)
    };
    format!(
        r#"<label class="setting-check">
  <input type="checkbox" name="{name}" style="width:auto" {c} />
  <span><strong>{label}</strong> {hint_html}</span>
</label>"#
    )
}

pub async fn settings_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let all = settings::get_all_server_settings(&web.app.db)
        .await
        .unwrap_or_default();
    let board_list = boards::list_all(&web.app.db).await.unwrap_or_default();
    let engines = search_engines::list_all(&web.app.db)
        .await
        .unwrap_or_default();

    let board_s = all.get("board");
    let home_id = board_s
        .and_then(|v| v.get("homeBoardId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mob_id = board_s
        .and_then(|v| v.get("mobileHomeBoardId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let status_def = board_s
        .and_then(|v| v.get("enableStatusByDefault"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let force_dis = board_s
        .and_then(|v| v.get("forceDisableStatus"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let board_opts = |sel: &str| -> String {
        let mut s = String::from(r#"<option value="">None (First visible board)</option>"#);
        for b in &board_list {
            if b.is_public {
                let checked = if b.id == sel { " selected" } else { "" };
                s.push_str(&format!(
                    r#"<option value="{}"{checked}>{}</option>"#,
                    encode_attr(&b.id),
                    encode_text(&b.name)
                ));
            }
        }
        s
    };

    let branding = all
        .get("branding")
        .map(parse_branding_settings)
        .unwrap_or_default();
    let def_scheme = all
        .get("appearance")
        .and_then(|v| v.get("defaultColorScheme"))
        .and_then(Value::as_str)
        .unwrap_or("dark");
    let mut scheme_opts = String::new();
    for (val, lbl) in [
        ("dark", "Dark"),
        ("light", "Light"),
        ("auto", "Auto (System)"),
    ] {
        let sel = if val == def_scheme { " selected" } else { "" };
        scheme_opts.push_str(&format!(r#"<option value="{val}"{sel}>{lbl}</option>"#));
    }

    let def_se_id = all
        .get("search")
        .and_then(|v| v.get("defaultSearchEngineId"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut se_opts = String::from(r#"<option value="">None (DuckDuckGo fallback)</option>"#);
    for se in &engines {
        let sel = if se.id == def_se_id { " selected" } else { "" };
        se_opts.push_str(&format!(
            r#"<option value="{}"{sel}>{} (!{})</option>"#,
            encode_attr(&se.id),
            encode_text(&se.name),
            encode_text(&se.short)
        ));
    }

    let def_locale = all
        .get("culture")
        .and_then(|v| v.get("defaultLocale"))
        .and_then(Value::as_str)
        .unwrap_or("en");
    let mut locale_opts = String::new();
    for code in web.i18n.codes() {
        let sel = if code == def_locale { " selected" } else { "" };
        locale_opts.push_str(&format!(
            r#"<option value="{}"{sel}>{}</option>"#,
            encode_attr(code),
            encode_text(code)
        ));
    }

    let gravatar = all
        .get("user")
        .and_then(|v| v.get("enableGravatar"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let analytics = all
        .get("analytics")
        .and_then(|v| v.get("enableGeneral"))
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let crawl = all.get("crawlingAndIndexing");
    let no_index = crawl
        .and_then(|v| v.get("noIndex"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let no_follow = crawl
        .and_then(|v| v.get("noFollow"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let no_translate = crawl
        .and_then(|v| v.get("noTranslate"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let no_sitelinks = crawl
        .and_then(|v| v.get("noSiteLinksSearchBox"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let content = format!(
        r#"<div class="page-header">
  <div>
    <h1 class="page-title">Server Settings</h1>
    <p class="muted" style="margin:4px 0 0">Configure instance-wide home boards, branding, localization, and crawler directives.</p>
  </div>
</div>
<form class="settings-stack" data-settings-form="/api/settings">
  <div class="card">
    <h3 class="settings-section-title">Boards &amp; Status</h3>
    <div class="settings-two-col">
      <div class="form-row"><label>Global Home Board</label><select name="board.homeBoardId">{home_opts}</select></div>
      <div class="form-row"><label>Global Mobile Board</label><select name="board.mobileHomeBoardId">{mob_opts}</select></div>
    </div>
    <div class="settings-check-list">
      {chk_status_def}
      {chk_force_dis}
    </div>
  </div>
  <div class="card">
    <h3 class="settings-section-title">Appearance &amp; Branding</h3>
    <div class="settings-two-col">
      <div class="form-row"><label>Application Name</label><input name="branding.appName" value="{app_name}" maxlength="80" placeholder="Homarr" /></div>
      <div class="form-row"><label>Default Color Scheme</label><select name="appearance.defaultColorScheme">{scheme_opts}</select></div>
      <div class="form-row"><label>Logo Image URL</label><input name="branding.logoImageUrl" value="{logo_url}" placeholder="/logo/homarr.svg" /></div>
      <div class="form-row"><label>Favicon Image URL</label><input name="branding.faviconImageUrl" value="{favicon_url}" placeholder="/favicon.ico" /></div>
    </div>
    <div class="form-row" style="margin-top:10px"><label>Login Page Greeting</label><input name="branding.greeting" value="{greeting}" maxlength="160" placeholder="Welcome to your homelab workspace" /></div>
  </div>
  <div class="card">
    <h3 class="settings-section-title">Search &amp; Localization</h3>
    <div class="settings-two-col">
      <div class="form-row"><label>Default Search Engine</label><select name="search.defaultSearchEngineId">{se_opts}</select></div>
      <div class="form-row"><label>Default Language</label><select name="culture.defaultLocale">{locale_opts}</select></div>
    </div>
  </div>
  <div class="card">
    <h3 class="settings-section-title">Users &amp; Analytics</h3>
    <div class="settings-check-list">
      {chk_gravatar}
      {chk_analytics}
    </div>
  </div>
  <div class="card">
    <h3 class="settings-section-title">Crawling &amp; Indexing</h3>
    <div class="settings-check-list">
      {chk_noindex}
      {chk_nofollow}
      {chk_notranslate}
      {chk_sitelinks}
    </div>
  </div>
  <div style="display:flex;justify-content:flex-end">
    <button type="submit" class="btn btn-primary">Save Settings</button>
  </div>
</form>"#,
        home_opts = board_opts(home_id),
        mob_opts = board_opts(mob_id),
        chk_status_def = check_row(
            "board.enableStatusByDefault",
            "Enable app status by default",
            status_def,
            "Ping new app items automatically",
        ),
        chk_force_dis = check_row(
            "board.forceDisableStatus",
            "Force disable app status pings",
            force_dis,
            "Disable status checks across all boards",
        ),
        app_name = encode_attr(&branding.app_name),
        logo_url = encode_attr(branding.logo_image_url.as_deref().unwrap_or("")),
        favicon_url = encode_attr(branding.favicon_image_url.as_deref().unwrap_or("")),
        greeting = encode_attr(&branding.greeting),
        chk_gravatar = check_row(
            "user.enableGravatar",
            "Enable Gravatar fallback avatars",
            gravatar,
            "",
        ),
        chk_analytics = check_row(
            "analytics.enableGeneral",
            "Enable general telemetry counter",
            analytics,
            "",
        ),
        chk_noindex = check_row(
            "crawlingAndIndexing.noIndex",
            "No index (noindex)",
            no_index,
            "Advise search engines not to index this instance",
        ),
        chk_nofollow = check_row(
            "crawlingAndIndexing.noFollow",
            "No follow (nofollow)",
            no_follow,
            "",
        ),
        chk_notranslate = check_row(
            "crawlingAndIndexing.noTranslate",
            "No translate (notranslate)",
            no_translate,
            "",
        ),
        chk_sitelinks = check_row(
            "crawlingAndIndexing.noSiteLinksSearchBox",
            "Hide search box sitelinks",
            no_sitelinks,
            "",
        ),
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/settings",
        "Settings",
        &content,
    )
    .await
    .into_response()
}

pub async fn search_engines_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let engines = search_engines::list_all(&web.app.db)
        .await
        .unwrap_or_default();
    let mut rows = String::new();
    for se in &engines {
        let id = encode_text(&se.id);
        let name = encode_text(&se.name);
        let short = encode_text(&se.short);
        let tpl = encode_text(se.url_template.as_deref().unwrap_or("-"));
        let icon = encode_attr(safe_url(&se.icon_url));
        rows.push_str(&format!(
            r#"<tr>
  <td><div class="table-name-cell"><img class="table-app-icon" src="{icon}" alt="" loading="lazy" onerror="this.onerror=null;this.src='/logo/homarr.svg'" /><span><strong>{name}</strong></span></div></td>
  <td><code>!{short}</code></td>
  <td><code>{tpl}</code></td>
  <td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/search-engines/{id}" data-method="DELETE">Delete</button></td>
</tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(4, "No search engines configured yet.");
    }
    let content = format!(
        r##"<div class="page-header"><h1 class="page-title">Search Engines</h1></div>
<div class="card">
  <h3 style="margin-top:0">Add Search Engine</h3>
  <form class="form-grid" style="max-width:none" data-api-form="/api/search-engines">
    <div class="settings-two-col">
      <div class="form-row"><label>Name</label><input name="name" required placeholder="Startpage" /></div>
      <div class="form-row"><label>Shortcut Prefix (without !)</label><input name="short" required placeholder="sp" /></div>
    </div>
    <div class="settings-two-col">
      <div class="form-row"><label>URL Template (%s placeholder)</label><input name="urlTemplate" required placeholder="https://www.startpage.com/sp/search?q=%s" /></div>
      <div class="form-row"><label>Icon URL</label><input name="iconUrl" required value="/logo/homarr.svg" /></div>
    </div>
    <div class="form-row"><label>Description (optional)</label><input name="description" placeholder="Private web search" /></div>
    <div><button type="submit" class="btn btn-primary">Add Search Engine</button></div>
  </form>
</div>
<div class="card">
  <div class="table-toolbar">
    <input type="search" class="table-search-input" data-table-filter="#se-table" placeholder="Search engines..." aria-label="Filter search engines" />
  </div>
  <div class="table-wrap"><table id="se-table"><thead><tr><th>Engine</th><th>Prefix</th><th>URL Template</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>
</div>"##
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/search-engines",
        "Search Engines",
        &content,
    )
    .await
    .into_response()
}

pub async fn api_reference_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let spec = openapi_spec().await.0;
    let mut rows = String::new();
    if let Some(paths) = spec.get("paths").and_then(Value::as_object) {
        for (path, methods_val) in paths {
            let Some(methods) = methods_val.as_object() else {
                continue;
            };
            for (method, meta) in methods {
                let m_upper = method.to_ascii_uppercase();
                let m_lower = method.to_ascii_lowercase();
                let summary = meta.get("summary").and_then(Value::as_str).unwrap_or("");
                rows.push_str(&format!(
                    r#"<div class="api-ep-row">
  <span class="http-badge http-{m_lower}">{m_upper}</span>
  <code class="api-ep-path">{}</code>
  <span class="muted api-ep-summary">{}</span>
</div>"#,
                    encode_text(path),
                    encode_text(summary)
                ));
            }
        }
    }

    let content = format!(
        r##"<div class="page-header">
  <div>
    <h1 class="page-title">REST, WebSocket &amp; MCP API Reference</h1>
    <p class="muted" style="margin:4px 0 0">OpenAPI 3.1.0 specification, authentication headers, and live endpoints.</p>
  </div>
  <div style="display:flex;gap:8px;flex-wrap:wrap">
    <a class="btn btn-sm" href="/api/openapi" download="homarr-openapi.json">Download OpenAPI JSON</a>
    <a class="btn btn-sm" href="/manage/tools/api-keys">Manage API Keys</a>
  </div>
</div>
<div class="cards-grid">
  <div class="card" style="margin-bottom:0">
    <h3 style="margin-top:0">Authentication</h3>
    <p class="muted" style="font-size:.86rem;margin-bottom:8px">Authenticate via session cookie, API key header, or bearer token:</p>
    <div><code>ApiKey: &lt;id&gt;.&lt;secret&gt;</code></div>
    <div style="margin-top:6px"><code>Authorization: Bearer &lt;token&gt;</code></div>
  </div>
  <div class="card" style="margin-bottom:0">
    <h3 style="margin-top:0">MCP &amp; WebSocket</h3>
    <p class="muted" style="font-size:.86rem;margin-bottom:8px">Connect Model Context Protocol clients and live socket listeners:</p>
    <div><strong>MCP:</strong> <code>POST /api/mcp</code> (JSON-RPC 2.0)</div>
    <div style="margin-top:6px"><strong>WebSocket:</strong> <code>GET /websockets</code></div>
  </div>
</div>
<div class="card" style="margin-top:16px">
  <div class="table-toolbar" style="justify-content:space-between;display:flex;align-items:center;gap:12px;flex-wrap:wrap">
    <h3 style="margin:0">OpenAPI 3.1 Endpoints</h3>
    <input type="search" class="table-search-input" data-list-filter="#openapi-list" placeholder="Filter endpoints (e.g. /api/boards, POST)..." aria-label="Filter API endpoints" />
  </div>
  <div id="openapi-list" class="api-ep-list">{rows}</div>
</div>"##
    );

    if session.as_ref().is_some_and(AuthSession::is_admin) {
        return render_manage_shell(
            &web,
            session.as_ref(),
            &headers,
            "/reference/api",
            "API Reference",
            &content,
        )
        .await
        .into_response();
    }

    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let visible = super::board::list_visible_boards(state, session.as_ref()).await;
    let spotlight = build_spotlight_items(state, &visible, false, false).await;
    let ui = ServerUiSettings::load(&state.db).await;
    let mut ctx = PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        spotlight_items: &spotlight,
        ..PageContext::minimal("API Reference", &content, &locale, &web.i18n)
    };
    ui.apply(&mut ctx);
    Html(render_page(ctx)).into_response()
}
