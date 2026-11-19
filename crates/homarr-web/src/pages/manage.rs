use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use homarr_auth::{AuthSession, OptionalAuth};
use homarr_db::repos::{
    api_keys, apps, assistant, boards, custom_widgets, groups, integrations, invites,
    search_engines, users,
};
use homarr_definitions::{integration::all_integration_defs, WidgetKind};
use homarr_widgets::{all_widget_metadata, get_widget_metadata, render_widget_html, safe_url};
use html_escape::{encode_double_quoted_attribute as encode_attr, encode_text};
use serde_json::json;

use super::board_editor::script_json;
use super::manage_extra::integration_defs_map_json;
pub use super::manage_extra::{api_reference_page, search_engines_page, settings_page};
use crate::icons;
use crate::layout::{build_spotlight_items, render_page, PageContext, ServerUiSettings};
use crate::WebState;

struct NavGroup {
    heading: &'static str,
    links: &'static [(&'static str, &'static str, &'static str)],
}

const NAV_GROUPS: &[NavGroup] = &[
    NavGroup {
        heading: "Workspace",
        links: &[
            ("/manage", "Overview", icons::HOME),
            ("/manage/boards", "Boards", icons::LAYOUT_DASHBOARD),
            ("/manage/apps", "Apps", icons::APPS),
            ("/manage/integrations", "Integrations", icons::PLUG),
            ("/manage/search-engines", "Search Engines", icons::SEARCH),
            ("/manage/custom-widgets", "Custom Widgets", icons::CODE),
            ("/manage/settings", "Settings", icons::SLIDERS),
        ],
    },
    NavGroup {
        heading: "Users & Access",
        links: &[
            ("/manage/users", "Users", icons::USERS),
            ("/manage/users/groups", "Groups", icons::USERS_GROUP),
            ("/manage/users/invites", "Invites", icons::MAIL),
        ],
    },
    NavGroup {
        heading: "Tools",
        links: &[
            ("/manage/tools/docker", "Docker", icons::DOCKER),
            ("/manage/tools/kubernetes", "Kubernetes", icons::SERVER),
            ("/manage/tools/tasks", "Tasks", icons::CLOCK),
            ("/manage/tools/logs", "Logs", icons::FILE_TEXT),
            (
                "/manage/tools/certificates",
                "Certificates",
                icons::CERTIFICATE,
            ),
            ("/manage/tools/api-keys", "API Keys", icons::KEY),
            ("/manage/tools/backup", "Backup", icons::DATABASE),
        ],
    },
    NavGroup {
        heading: "System",
        links: &[
            ("/manage/assistant", "Assistant", icons::ROBOT),
            ("/manage/about", "About", icons::INFO),
            ("/reference/api", "API Docs", icons::BOOK),
        ],
    },
];

pub(super) fn empty_table_row(colspan: usize, message: &str) -> String {
    format!(
        r#"<tr><td colspan="{colspan}" class="muted" style="text-align:center;padding:24px">{}</td></tr>"#,
        encode_text(message)
    )
}

pub(super) async fn render_manage_shell(
    web: &WebState,
    session: Option<&AuthSession>,
    headers: &HeaderMap,
    active_path: &str,
    title: &str,
    content: &str,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(headers);
    let visible_boards = boards::list_all(&state.db).await.unwrap_or_default();
    let is_admin = session.is_some_and(AuthSession::is_admin);
    let spotlight = build_spotlight_items(state, &visible_boards, is_admin, false).await;
    let ui = ServerUiSettings::load(&state.db).await;

    let mut sidebar = format!(
        r#"<aside class="sidebar" id="manage-sidebar">
  <div class="sidebar-mobile-head">
    <strong>Navigation</strong>
    <button type="button" class="icon-btn icon-btn-sm" onclick="window.closeManageDrawer()" aria-label="Close menu">{close}</button>
  </div>
  <nav class="sidebar-nav">"#,
        close = icons::X
    );
    for group in NAV_GROUPS {
        sidebar.push_str(&format!(
            r#"<div class="sidebar-group-title">{}</div>"#,
            group.heading
        ));
        for (href, label, icon) in group.links {
            let cls = if *href == active_path {
                "sidebar-link active"
            } else {
                "sidebar-link"
            };
            sidebar.push_str(&format!(
                r#"<a class="{cls}" href="{href}">{icon}<span>{label}</span></a>"#
            ));
        }
    }
    sidebar.push_str("</nav></aside>");

    let full_body = format!(
        r#"<div class="manage-mobile-bar">
  <button type="button" class="btn btn-sm" onclick="window.toggleManageDrawer()">{menu}<span>Menu</span></button>
  <span class="muted" style="font-size:.84rem">{title_enc}</span>
</div>
<div class="manage-drawer-backdrop" onclick="window.closeManageDrawer()"></div>
<div class="manage-shell">{sidebar}<div class="manage-content">{content}</div></div>"#,
        menu = icons::MENU,
        title_enc = encode_text(title),
    );

    let mut ctx = PageContext {
        session,
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible_boards,
        spotlight_items: &spotlight,
        ..PageContext::minimal(title, &full_body, &locale, &web.i18n)
    };
    ui.apply(&mut ctx);
    Html(render_page(ctx))
}

pub(super) fn require_admin_or_redirect(
    session: Option<&AuthSession>,
) -> Result<&AuthSession, Redirect> {
    match session {
        Some(s) if s.is_admin() => Ok(s),
        _ => Err(Redirect::temporary("/auth/login")),
    }
}

pub async fn overview(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let sess = match require_admin_or_redirect(session.as_ref()) {
        Ok(s) => s,
        Err(redir) => return redir.into_response(),
    };
    let state = &web.app;
    let boards_cnt = boards::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let apps_cnt = apps::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let ints_cnt = integrations::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let users_cnt = usize::try_from(users::count(&state.db).await.unwrap_or(0)).unwrap_or(0);
    let groups_cnt = groups::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let invites_cnt = invites::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let se_cnt = search_engines::list_all(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let cw_cnt = custom_widgets::list_v2(&state.db)
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    let uname = encode_text(sess.username.as_deref().unwrap_or("admin"));
    let kpis = [
        (
            "/manage/boards",
            "Boards",
            "Boards",
            boards_cnt,
            "kpi-red",
            icons::LAYOUT_DASHBOARD,
        ),
        (
            "/manage/apps",
            "Apps",
            "Resources",
            apps_cnt,
            "kpi-blue",
            icons::APPS,
        ),
        (
            "/manage/integrations",
            "Integrations",
            "Resources",
            ints_cnt,
            "kpi-green",
            icons::PLUG,
        ),
        (
            "/manage/users",
            "Users",
            "Authentication",
            users_cnt,
            "kpi-yellow",
            icons::USERS,
        ),
        (
            "/manage/users/groups",
            "Groups",
            "Authorization",
            groups_cnt,
            "kpi-blue",
            icons::USERS_GROUP,
        ),
        (
            "/manage/users/invites",
            "Invites",
            "Authentication",
            invites_cnt,
            "kpi-red",
            icons::MAIL,
        ),
        (
            "/manage/search-engines",
            "Search Engines",
            "Resources",
            se_cnt,
            "kpi-green",
            icons::SEARCH,
        ),
        (
            "/manage/custom-widgets",
            "Custom Widgets",
            "Widgets",
            cw_cnt,
            "kpi-yellow",
            icons::CODE,
        ),
    ];

    let mut kpi_html = String::new();
    for (href, label, sub, count, color_cls, icon) in kpis {
        kpi_html.push_str(&format!(
            r#"<a class="card kpi-card" href="{href}">
  <div class="kpi-icon {color_cls}">{icon}</div>
  <div style="flex:1;min-width:0">
    <div class="metric-big">{count}</div>
    <div style="font-weight:600;color:var(--text-strong);font-size:.9rem">{label}</div>
    <div class="muted" style="font-size:.74rem">{sub}</div>
  </div>
  <span class="kpi-arrow">{arrow}</span>
</a>"#,
            arrow = icons::ARROW_UP_RIGHT
        ));
    }

    let content = format!(
        r#"<div class="card manage-hero">
  <div style="display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap">
    <div>
      <h1 class="page-title" style="font-size:1.7rem">Welcome back, <span style="color:var(--primary)">{uname}</span></h1>
      <p class="muted" style="margin:6px 0 0">Everything you manage in Homarr, in one place.</p>
    </div>
    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <a class="btn btn-primary" href="/manage/boards">{plus_icon}<span>Create Board</span></a>
      <a class="btn" href="/manage/apps">{apps_icon}<span>Add App</span></a>
      <a class="btn" href="/manage/integrations">{plug_icon}<span>Add Integration</span></a>
      <a class="btn" href="/manage/settings">{sliders_icon}<span>Settings</span></a>
    </div>
  </div>
</div>
<div class="cards-grid">{kpi_html}</div>
<div class="card">
  <h3 style="margin-top:0">Tools &amp; System</h3>
  <div style="display:flex;gap:10px;flex-wrap:wrap">
    <a class="btn btn-sm" href="/manage/tools/tasks">{clock_icon}<span>Cron Tasks</span></a>
    <a class="btn btn-sm" href="/manage/tools/docker">{docker_icon}<span>Docker</span></a>
    <a class="btn btn-sm" href="/manage/tools/logs">{logs_icon}<span>Server Logs</span></a>
    <a class="btn btn-sm" href="/manage/tools/api-keys">{key_icon}<span>API Keys</span></a>
    <a class="btn btn-sm" href="/manage/tools/backup">{db_icon}<span>Backup</span></a>
    <a class="btn btn-sm" href="/manage/assistant">{sparkles_icon}<span>AI Assistant</span></a>
    <a class="btn btn-sm" href="/reference/api">{book_icon}<span>API Docs</span></a>
  </div>
</div>"#,
        apps_icon = icons::APPS,
        plug_icon = icons::PLUG,
        sliders_icon = icons::SLIDERS,
        plus_icon = icons::PLUS,
        clock_icon = icons::CLOCK,
        docker_icon = icons::DOCKER,
        logs_icon = icons::FILE_TEXT,
        key_icon = icons::KEY,
        db_icon = icons::DATABASE,
        sparkles_icon = icons::SPARKLES,
        book_icon = icons::BOOK,
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage",
        "Manage",
        &content,
    )
    .await
    .into_response()
}

pub async fn boards_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = boards::list_all(&web.app.db).await.unwrap_or_default();
    let mut rows = String::new();
    for b in &list {
        let name = encode_text(&b.name);
        let id = encode_text(&b.id);
        let vis = if b.is_public { "Public" } else { "Private" };
        rows.push_str(&format!(
            r#"<tr>
  <td><a href="/boards/{name}"><strong>{name}</strong></a></td>
  <td><span class="badge">{vis}</span></td>
  <td style="display:flex;gap:6px">
    <a class="btn btn-sm" href="/boards/{name}">{ext}<span>Open</span></a>
    <a class="btn btn-sm" href="/boards/{name}/settings">Settings</a>
    <button type="button" class="btn btn-danger btn-sm" data-api-action="/api/boards/{id}" data-method="DELETE">Delete</button>
  </td>
</tr>"#,
            ext = icons::EXTERNAL_LINK,
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No boards yet. Create your first board above.");
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Boards</h1></div>
<div class="card">
  <h3 style="margin-top:0">Create New Board</h3>
  <form class="form-grid" data-api-form="/api/boards">
    <div class="form-row"><label>Name</label><input name="name" required placeholder="Homelab" /></div>
    <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="isPublic" style="width:auto" checked /> Public</label>
    <button type="submit" class="btn btn-primary">Create Board</button>
  </form>
</div>
<div class="card table-wrap">
  <table><thead><tr><th>Name</th><th>Visibility</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table>
</div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/boards",
        "Manage Boards",
        &content,
    )
    .await
    .into_response()
}

pub async fn apps_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = apps::list_all(&web.app.db).await.unwrap_or_default();
    let mut rows = String::new();
    for a in &list {
        let name = encode_text(&a.name);
        let desc = encode_text(a.description.as_deref().unwrap_or(""));
        let id = encode_text(&a.id);
        let icon = encode_attr(safe_url(&a.icon_url));
        let href_safe = a.href.as_deref().map(safe_url).filter(|u| *u != "#");
        let href_cell = match href_safe {
            Some(u) => format!(
                r#"<a href="{}" target="_blank" rel="noopener noreferrer"><code>{}</code></a>"#,
                encode_attr(u),
                encode_text(u)
            ),
            None => r#"<span class="muted">-</span>"#.to_string(),
        };
        let app_payload_json = json!({
            "id": a.id,
            "name": a.name,
            "description": a.description,
            "iconUrl": a.icon_url,
            "href": a.href,
            "pingUrl": a.ping_url,
        })
        .to_string();
        let app_payload = encode_attr(&app_payload_json);
        rows.push_str(&format!(
            r#"<tr>
  <td><div class="table-name-cell"><img class="table-app-icon" src="{icon}" alt="" loading="lazy" onerror="this.onerror=null;this.src='/logo/homarr.svg'" /><div><strong>{name}</strong><div class="muted" style="font-size:.76rem">{desc}</div></div></div></td>
  <td>{href_cell}</td>
  <td style="display:flex;gap:6px">
    <button type="button" class="btn btn-sm" data-edit-app="{app_payload}">Edit</button>
    <button type="button" class="btn btn-danger btn-sm" data-api-action="/api/apps/{id}" data-method="DELETE">Delete</button>
  </td>
</tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No applications yet. Add your first application above.");
    }
    let content = format!(
        r##"<div class="page-header"><h1 class="page-title">Applications</h1></div>
<div class="card">
  <h3 style="margin-top:0">Add Application</h3>
  <form class="form-grid" style="max-width:none" data-api-form="/api/apps">
    <div class="settings-two-col">
      <div class="form-row"><label>Name</label><input name="name" required placeholder="Sonarr" /></div>
      <div class="form-row"><label>URL</label><input name="href" placeholder="https://sonarr.local" /></div>
      <div class="form-row"><label>Icon URL</label><input id="app-icon-url-input" name="iconUrl" required value="/logo/homarr.svg" /></div>
      <div class="form-row"><label>Description</label><input name="description" placeholder="Optional description" /></div>
    </div>
    <div class="form-row">
      <label>Search Bundled Icons</label>
      <input type="search" data-icon-search="#app-icon-picker" data-icon-target="#app-icon-url-input" placeholder="Type e.g. sonarr, grafana, plex, nextcloud..." autocomplete="off" />
      <div id="app-icon-picker" class="editor-icon-results"></div>
    </div>
    <div><button type="submit" class="btn btn-primary">Add Application</button></div>
  </form>
</div>
<div class="card">
  <div class="table-toolbar">
    <input type="search" class="table-search-input" data-table-filter="#apps-table" placeholder="Search applications..." aria-label="Filter applications" />
  </div>
  <div class="table-wrap"><table id="apps-table"><thead><tr><th>Application</th><th>URL</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>
</div>"##
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/apps",
        "Manage Apps",
        &content,
    )
    .await
    .into_response()
}

pub async fn integrations_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = integrations::list_all(&web.app.db)
        .await
        .unwrap_or_default();
    let mut kinds_opts = String::new();
    for def in all_integration_defs() {
        kinds_opts.push_str(&format!(
            r#"<option value="{}">{}</option>"#,
            encode_attr(def.kind_str),
            encode_text(def.name)
        ));
    }
    let mut rows = String::new();
    for int in &list {
        let id = encode_text(&int.id);
        let name = encode_text(&int.name);
        let kind = encode_text(&int.kind);
        let url = encode_text(&int.url);
        rows.push_str(&format!(
            r#"<tr>
  <td><strong>{name}</strong></td><td><span class="badge">{kind}</span></td><td><code>{url}</code></td>
  <td style="display:flex;gap:6px">
    <button type="button" class="btn btn-sm" data-api-action="/api/integrations/{id}/test" data-method="POST" data-reload="false">Test</button>
    <button type="button" class="btn btn-danger btn-sm" data-api-action="/api/integrations/{id}" data-method="DELETE">Delete</button>
  </td>
</tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(4, "No integrations connected yet.");
    }
    let defs_json = script_json(&integration_defs_map_json());
    let content = format!(
        r##"<div class="page-header"><h1 class="page-title">Integrations</h1></div>
<div class="card">
  <h3 style="margin-top:0">Connect Integration</h3>
  <form class="form-grid" style="max-width:none" data-api-form="/api/integrations" id="integration-create-form">
    <div class="settings-two-col">
      <div class="form-row"><label>Kind (92 Supported)</label><select name="kind" id="integration-kind-select">{kinds_opts}</select></div>
      <div class="form-row"><label>Name</label><input name="name" required placeholder="My Service" /></div>
    </div>
    <div class="form-row"><label>URL</label><input name="url" id="integration-url-input" required placeholder="http://127.0.0.1:8989" /></div>
    <div id="integration-secret-fields" class="settings-two-col"></div>
    <details class="editor-details">
      <summary>Advanced: Raw Secrets JSON</summary>
      <div class="form-row" style="margin-top:8px"><input name="secretsJson" placeholder='[{{"kind":"apiKey","value":"..."}}]' /></div>
    </details>
    <div><button type="submit" class="btn btn-primary">Save Integration</button></div>
  </form>
</div>
<div class="card">
  <div class="table-toolbar">
    <input type="search" class="table-search-input" data-table-filter="#integrations-table" placeholder="Search integrations..." aria-label="Filter integrations" />
  </div>
  <div class="table-wrap"><table id="integrations-table"><thead><tr><th>Name</th><th>Kind</th><th>URL</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>
</div>
<script>window.__HOMARR_INTEGRATION_DEFS__ = {defs_json};</script>"##
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/integrations",
        "Integrations",
        &content,
    )
    .await
    .into_response()
}

pub async fn users_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = users::list_all(&web.app.db).await.unwrap_or_default();
    let mut rows = String::new();
    for u in &list {
        let id = encode_text(&u.id);
        let name = encode_text(u.name.as_deref().unwrap_or(""));
        let email = encode_text(u.email.as_deref().unwrap_or("-"));
        let provider = encode_text(&u.provider);
        rows.push_str(&format!(
            r#"<tr><td><strong>{name}</strong></td><td>{email}</td><td><span class="badge">{provider}</span></td>
<td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/users/{id}" data-method="DELETE">Delete</button></td></tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(4, "No users found.");
    }
    let content = format!(
        r##"<div class="page-header"><h1 class="page-title">Users</h1></div>
<div class="card">
  <h3 style="margin-top:0">Create User</h3>
  <form class="form-grid" style="max-width:none" data-api-form="/api/users">
    <div class="settings-two-col">
      <div class="form-row"><label>Username</label><input name="name" required minlength="2" /></div>
      <div class="form-row"><label>Email</label><input name="email" type="email" /></div>
    </div>
    <div class="form-row"><label>Password</label><input name="password" type="password" required minlength="4" /></div>
    <div><button type="submit" class="btn btn-primary">Create User</button></div>
  </form>
</div>
<div class="card">
  <div class="table-toolbar">
    <input type="search" class="table-search-input" data-table-filter="#users-table" placeholder="Search users..." aria-label="Filter users" />
  </div>
  <div class="table-wrap"><table id="users-table"><thead><tr><th>Username</th><th>Email</th><th>Provider</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>
</div>"##
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/users",
        "Users",
        &content,
    )
    .await
    .into_response()
}

pub async fn groups_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = groups::list_all(&web.app.db).await.unwrap_or_default();
    let mut rows = String::new();
    for g in &list {
        let id = encode_text(&g.id);
        let name = encode_text(&g.name);
        let perms = groups::get_group_permissions(&web.app.db, &g.id)
            .await
            .unwrap_or_default()
            .join(", ");
        rows.push_str(&format!(
            r#"<tr><td><strong>{name}</strong></td><td><code>{}</code></td><td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/groups/{id}" data-method="DELETE">Delete</button></td></tr>"#,
            encode_text(&perms)
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No groups found.");
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Groups</h1></div>
<div class="card">
  <h3 style="margin-top:0">Create Group</h3>
  <form class="form-grid" data-api-form="/api/groups">
    <div class="form-row"><label>Group Name</label><input name="name" required /></div>
    <button type="submit" class="btn btn-primary">Create Group</button>
  </form>
</div>
<div class="card table-wrap"><table><thead><tr><th>Name</th><th>Permissions</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/users/groups",
        "Groups",
        &content,
    )
    .await
    .into_response()
}

pub async fn invites_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = invites::list_all(&web.app.db).await.unwrap_or_default();
    let mut rows = String::new();
    for inv in &list {
        let id = encode_text(&inv.id);
        let tok = encode_text(&inv.token);
        rows.push_str(&format!(
            r#"<tr><td><code>/auth/invite/{tok}</code></td><td>{}</td><td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/invites/{id}" data-method="DELETE">Revoke</button></td></tr>"#,
            inv.expiration_date
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No active invites.");
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Invites</h1></div>
<div class="card"><button type="button" class="btn btn-primary" data-api-action="/api/invites" data-method="POST">Generate Invite Link</button></div>
<div class="card table-wrap"><table><thead><tr><th>Invite Path</th><th>Expires (Unix)</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/users/invites",
        "Invites",
        &content,
    )
    .await
    .into_response()
}

pub async fn custom_widgets_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let list = custom_widgets::list_v2(&web.app.db)
        .await
        .unwrap_or_default();
    let mut rows = String::new();
    for cw in &list {
        let id = encode_text(&cw.id);
        let name = encode_text(&cw.name);
        rows.push_str(&format!(
            r#"<tr><td><strong>{name}</strong></td><td><span class="badge">{}</span></td><td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/custom-widgets/{id}" data-method="DELETE">Delete</button></td></tr>"#,
            if cw.enabled { "Enabled" } else { "Disabled" }
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No custom widgets created yet.");
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Custom Widgets v2</h1></div>
<div class="card">
  <h3 style="margin-top:0">Create Custom Widget</h3>
  <form class="form-grid" data-api-form="/api/custom-widgets">
    <div class="form-row"><label>Name</label><input name="name" required placeholder="My Status Card" /></div>
    <div class="form-row"><label>Template HTML (XSS-escaped placeholders)</label><textarea name="template" rows="4" required>&lt;div&gt;{{title}}&lt;/div&gt;</textarea></div>
    <button type="submit" class="btn btn-primary">Save Custom Widget</button>
  </form>
</div>
<div class="card table-wrap"><table><thead><tr><th>Name</th><th>Status</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/custom-widgets",
        "Custom Widgets",
        &content,
    )
    .await
    .into_response()
}

pub async fn docker_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let containers = web
        .app
        .services
        .docker
        .list_containers()
        .await
        .unwrap_or_default();
    let mut rows = String::new();
    for c in &containers {
        let id = encode_text(&c.id);
        let name = encode_text(&c.name);
        let image = encode_text(&c.image);
        let st = encode_text(&c.state);
        rows.push_str(&format!(
            r#"<tr><td><strong>{name}</strong></td><td><code>{image}</code></td><td><span class="badge">{st}</span></td>
<td style="display:flex;gap:4px">
  <button type="button" class="btn btn-sm" data-api-action="/api/docker/containers/{id}/restart" data-method="POST">Restart</button>
  <button type="button" class="btn btn-sm" data-api-action="/api/docker/containers/{id}/stop" data-method="POST">Stop</button>
</td></tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(
            4,
            "No Docker containers reported (or Docker socket is unavailable in this environment).",
        );
    }
    let notice = if containers.is_empty() {
        r#"<div class="card notice-warn">Docker socket <code>/var/run/docker.sock</code> is unavailable or no containers were returned.</div>"#
    } else {
        ""
    };
    let content = format!(
        r##"<div class="page-header">
  <h1 class="page-title">Docker Containers</h1>
  <button type="button" class="btn btn-sm" onclick="location.reload()">Refresh</button>
</div>
{notice}
<div class="card">
  <div class="table-toolbar">
    <input type="search" class="table-search-input" data-table-filter="#docker-table" placeholder="Search containers..." aria-label="Filter containers" />
  </div>
  <div class="table-wrap"><table id="docker-table"><thead><tr><th>Name</th><th>Image</th><th>State</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>
</div>"##
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/docker",
        "Docker",
        &content,
    )
    .await
    .into_response()
}

pub async fn kubernetes_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let body = if web.app.services.kubernetes.is_enabled() {
        let pods = web
            .app
            .services
            .kubernetes
            .get_resource("pods")
            .await
            .unwrap_or_else(|e| json!({ "error": e.to_string() }));
        format!(
            r#"<div class="card"><pre class="generic-json">{}</pre></div>"#,
            encode_text(&serde_json::to_string_pretty(&pods).unwrap_or_default())
        )
    } else {
        r#"<div class="card"><p class="muted">Kubernetes integration is disabled. Set <code>ENABLE_KUBERNETES=true</code> inside a cluster pod to enable it.</p></div>"#.to_string()
    };
    let content =
        format!(r#"<div class="page-header"><h1 class="page-title">Kubernetes</h1></div>{body}"#);
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/kubernetes",
        "Kubernetes",
        &content,
    )
    .await
    .into_response()
}

pub async fn tasks_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let jobs = web.app.scheduler.list_jobs(false).await;
    let mut rows = String::new();
    for j in &jobs {
        let name = encode_text(&j.name);
        let name_attr = encode_attr(&j.name);
        let expr = encode_text(&j.cron_expression);
        let desc = encode_text(&j.description);
        let (badge_cls, enabled_lbl, toggle_lbl, next_val) = if j.is_enabled {
            ("up", "Enabled", "Pause", "false")
        } else {
            ("down", "Paused", "Enable", "true")
        };
        let run_btn = if !j.prevent_manual_execution {
            format!(
                r#"<button type="button" class="btn btn-sm" data-api-action="/api/cron-jobs/{name}/trigger" data-method="POST">Run Now</button>"#
            )
        } else {
            String::new()
        };
        let toggle_btn = format!(
            r#"<button type="button" class="btn btn-sm" data-cron-toggle="{name_attr}" data-enabled="{next_val}">{toggle_lbl}</button>"#
        );
        rows.push_str(&format!(
            r#"<tr>
  <td><strong>{name}</strong><div class="muted" style="font-size:.78rem">{desc}</div></td>
  <td><code>{expr}</code></td>
  <td><span class="badge {badge_cls}">{enabled_lbl}</span></td>
  <td style="display:flex;gap:6px">{run_btn}{toggle_btn}</td>
</tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(4, "No scheduled jobs registered.");
    }
    let content = format!(
        r#"<div class="page-header">
  <h1 class="page-title">Background Cron Tasks</h1>
  <button type="button" class="btn btn-sm" onclick="location.reload()">Refresh</button>
</div>
<div class="card table-wrap"><table><thead><tr><th>Task</th><th>Schedule</th><th>Status</th><th>Action</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/tasks",
        "Tasks",
        &content,
    )
    .await
    .into_response()
}

pub async fn logs_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let entries = web.app.logs.recent(150);
    let mut lines = String::new();
    for l in &entries {
        let lvl_lower = l.level.to_ascii_lowercase();
        let lvl_cls = match lvl_lower.as_str() {
            "error" => "log-level-error",
            "warn" | "warning" => "log-level-warn",
            _ => "log-level-info",
        };
        lines.push_str(&format!(
            r#"<div class="log-line" data-level="{lvl_attr}"><span class="log-time">{time}</span> <span class="badge {lvl_cls}">{lvl}</span> <span class="log-msg">{msg}</span></div>"#,
            lvl_attr = encode_attr(&lvl_lower),
            time = encode_text(&l.timestamp),
            lvl = encode_text(&l.level),
            msg = encode_text(&l.message),
        ));
    }
    if lines.is_empty() {
        lines =
            r#"<div class="muted" style="padding:16px">No server log entries recorded yet.</div>"#
                .to_string();
    }
    let content = format!(
        r#"<div class="page-header">
  <h1 class="page-title">Server Logs</h1>
  <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
    <select id="log-level-filter" aria-label="Filter log level" style="width:auto">
      <option value="all">All Levels</option>
      <option value="info">Info</option>
      <option value="warn">Warn</option>
      <option value="error">Error</option>
    </select>
    <input id="log-text-filter" type="search" placeholder="Search logs..." style="width:220px" aria-label="Search logs" />
    <button type="button" class="btn btn-sm" onclick="location.reload()">Refresh</button>
  </div>
</div>
<div class="log-terminal" id="log-terminal">{lines}</div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/logs",
        "Logs",
        &content,
    )
    .await
    .into_response()
}

pub async fn certificates_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let certs = web.app.certs.list().unwrap_or_default();
    let mut rows = String::new();
    for c in &certs {
        let name = encode_text(&c.file_name);
        rows.push_str(&format!(
            r#"<tr><td>{name}</td><td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/certificates/{name}" data-method="DELETE">Delete</button></td></tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(2, "No custom trusted certificates uploaded.");
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Trusted Certificates</h1></div>
<div class="card">
  <form class="form-grid" data-api-form="/api/certificates">
    <div class="form-row"><label>Filename (.crt or .pem)</label><input name="fileName" required placeholder="homelab-ca.crt" /></div>
    <div class="form-row"><label>PEM Content</label><textarea name="content" rows="5" required placeholder="-----BEGIN CERTIFICATE-----"></textarea></div>
    <button type="submit" class="btn btn-primary">Upload Certificate</button>
  </form>
</div>
<div class="card table-wrap"><table><thead><tr><th>Filename</th><th>Action</th></tr></thead><tbody>{rows}</tbody></table></div>"#
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/certificates",
        "Certificates",
        &content,
    )
    .await
    .into_response()
}

pub async fn api_keys_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let sess = match require_admin_or_redirect(session.as_ref()) {
        Ok(s) => s,
        Err(redir) => return redir.into_response(),
    };
    let keys = api_keys::list_by_user(&web.app.db, &sess.user_id)
        .await
        .unwrap_or_default();
    let mut rows = String::new();
    for k in &keys {
        let id = encode_text(&k.id);
        let ro = if k.read_only {
            "Read-Only"
        } else {
            "Read/Write"
        };
        rows.push_str(&format!(
            r#"<tr><td><code>{id}</code></td><td><span class="badge">{ro}</span></td><td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/api-keys/{id}" data-method="DELETE">Revoke</button></td></tr>"#
        ));
    }
    if rows.is_empty() {
        rows = empty_table_row(3, "No API keys created yet.");
    }
    let content = format!(
        r##"<div class="page-header"><h1 class="page-title">API Keys</h1></div>
<div id="api-key-reveal" class="card api-key-banner" hidden>
  <h3 style="margin-top:0">New API Key Generated</h3>
  <p class="muted" style="margin:4px 0 10px">Copy this token now. For security reasons, it will not be displayed again after closing.</p>
  <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
    <code id="api-key-value" style="padding:8px 12px;font-size:.92rem;user-select:all"></code>
    <button type="button" class="btn btn-sm btn-primary" id="copy-api-key-btn">{copy}<span>Copy Key</span></button>
    <button type="button" class="btn btn-sm" onclick="location.reload()">Done &amp; Refresh</button>
  </div>
</div>
<div class="card">
  <form class="form-grid" data-api-form="/api/api-keys" data-reveal-key="#api-key-reveal">
    <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="readOnly" style="width:auto" /> Read-Only Key</label>
    <button type="submit" class="btn btn-primary">Create API Key</button>
  </form>
</div>
<div class="card table-wrap"><table id="api-keys-table"><thead><tr><th>Key ID</th><th>Scope</th><th>Action</th></tr></thead><tbody>{rows}</tbody></table></div>"##,
        copy = icons::COPY,
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/api-keys",
        "API Keys",
        &content,
    )
    .await
    .into_response()
}

pub async fn backup_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let content = r#"<div class="page-header"><h1 class="page-title">Backup &amp; Restore</h1></div>
<div class="card">
  <h3 style="margin-top:0">Export Full Backup Bundle</h3>
  <p class="muted">Downloads a portable JSON backup containing boards, apps, search engines, groups, and settings.</p>
  <a class="btn btn-primary" href="/api/backup/export" download="homarr-backup.json">Download Backup JSON</a>
</div>
<div class="card">
  <h3 style="margin-top:0">Restore from Backup JSON</h3>
  <p class="muted">Select an exported <code>homarr-backup.json</code> file or paste JSON below to restore workspace data.</p>
  <div class="form-grid" style="max-width:none">
    <div class="form-row"><label>Upload Backup File (.json)</label><input id="backup-file-input" type="file" accept=".json,application/json" /></div>
    <div class="form-row"><label>Backup JSON Payload</label><textarea id="backup-json-input" rows="6" placeholder='{"boards": [...], "apps": [...]}'></textarea></div>
    <div><button type="button" class="btn btn-primary" data-restore-backup>Restore Backup</button></div>
  </div>
</div>"#;
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/tools/backup",
        "Backup",
        content,
    )
    .await
    .into_response()
}

pub async fn assistant_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let cfg = assistant::get_config(&web.app.db)
        .await
        .unwrap_or_else(|_| homarr_db::models::AssistantConfigRecord {
            id: "default".into(),
            enabled: false,
            web_search_enabled: false,
            provider: "openrouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model_discovery_path: None,
            encrypted_api_key: None,
            encrypted_headers: None,
            model_id: None,
            updated_at: 0,
        });
    let checked = if cfg.enabled { "checked" } else { "" };
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">AI Assistant</h1></div>
<div class="card">
  <form class="form-grid" data-api-form="/api/assistant/config" data-method="PUT">
    <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="enabled" style="width:auto" {checked} /> Enable Assistant</label>
    <div class="form-row"><label>Provider (openrouter, openai, anthropic, ollama)</label><input name="provider" value="{}" required /></div>
    <div class="form-row"><label>Base URL</label><input name="baseUrl" value="{}" required /></div>
    <div class="form-row"><label>Model ID</label><input name="modelId" value="{}" /></div>
    <div class="form-row"><label>API Key (leave blank to keep existing)</label><input name="apiKey" type="password" /></div>
    <button type="submit" class="btn btn-primary">Save Assistant Configuration</button>
  </form>
</div>"#,
        encode_text(&cfg.provider),
        encode_text(&cfg.base_url),
        encode_text(cfg.model_id.as_deref().unwrap_or("openai/gpt-4o-mini")),
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/assistant",
        "Assistant",
        &content,
    )
    .await
    .into_response()
}

pub async fn about_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let content = format!(
        r#"<div class="card about-hero">
  <img src="/logo/homarr.svg" alt="Homarr" style="width:64px;height:64px" />
  <div>
    <h1 class="page-title">Homarr <span class="demo-pill">v{version}</span></h1>
    <p class="muted" style="margin:6px 0 0">Static Pure-Rust Dashboard Server (Axum + SQLx + Tokio + Bollard)</p>
  </div>
</div>
<div class="cards-grid">
  <div class="card" style="margin-bottom:0">
    <h3 style="margin-top:0">Runtime &amp; API</h3>
    <div style="display:flex;gap:8px;flex-wrap:wrap;margin-bottom:10px">
      <span class="badge">Distroless musl</span>
      <span class="badge">SQLite / PostgreSQL</span>
      <span class="badge">92 Integrations</span>
      <span class="badge">67 Widgets</span>
    </div>
    <p class="muted" style="font-size:.86rem;margin:0">OpenAPI 3.1 spec at <a href="/api/openapi"><code>/api/openapi</code></a> and MCP JSON-RPC 2.0 at <code>POST /api/mcp</code>.</p>
  </div>
  <div class="card" style="margin-bottom:0">
    <h3 style="margin-top:0">Resources</h3>
    <div style="display:flex;gap:8px;flex-wrap:wrap">
      <a class="btn btn-sm" href="/reference/api">Interactive API Reference</a>
      <a class="btn btn-sm" href="https://homarr.dev/docs/getting-started" target="_blank" rel="noopener noreferrer">Documentation</a>
      <a class="btn btn-sm" href="https://github.com/homarr-labs/homarr" target="_blank" rel="noopener noreferrer">GitHub</a>
    </div>
  </div>
</div>
<div class="card table-wrap" style="margin-top:16px">
  <h3 style="margin-top:0">Keyboard Shortcuts</h3>
  <table>
    <thead><tr><th>Shortcut</th><th>Action</th></tr></thead>
    <tbody>
      <tr><td><kbd class="kbd">Ctrl</kbd> + <kbd class="kbd">K</kbd> / <kbd class="kbd">Cmd</kbd> + <kbd class="kbd">K</kbd></td><td>Open Spotlight Search</td></tr>
      <tr><td><kbd class="kbd">Esc</kbd></td><td>Close modal, Spotlight, or mobile menu drawer</td></tr>
      <tr><td><kbd class="kbd">&uarr;</kbd> / <kbd class="kbd">&darr;</kbd> / <kbd class="kbd">&crarr;</kbd></td><td>Navigate and open Spotlight results</td></tr>
    </tbody>
  </table>
</div>"#,
        version = env!("CARGO_PKG_VERSION")
    );
    render_manage_shell(
        &web,
        session.as_ref(),
        &headers,
        "/manage/about",
        "About",
        &content,
    )
    .await
    .into_response()
}

pub async fn widget_preview_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    Path(kind_str): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let state = &web.app;
    let Some(kind) = WidgetKind::from_str_opt(&kind_str) else {
        return (StatusCode::NOT_FOUND, "Unknown widget kind").into_response();
    };
    let meta = get_widget_metadata(kind);
    let data = state
        .widgets
        .resolve_widget_data(kind, &meta.default_options, &[])
        .await
        .unwrap_or_else(|_| json!({}));
    let card = render_widget_html(kind, "preview", &meta.default_options, &data);

    let mut links = String::new();
    for m in all_widget_metadata() {
        links.push_str(&format!(
            r#"<a class="badge" href="/widgets/{}">{}</a> "#,
            encode_text(&m.kind),
            encode_text(&m.name)
        ));
    }

    let body = format!(
        r#"<div class="page-header"><h1 class="page-title">Widget Preview: {}</h1></div>
<div class="grid-item widget-preview-box">{card}</div>
<div class="card" style="margin-top:16px"><h3 style="margin-top:0">All 67 Widgets</h3><div style="display:flex;gap:6px;flex-wrap:wrap">{links}</div></div>"#,
        encode_text(&meta.name)
    );

    let locale = web.i18n.negotiate(&headers);
    let ui = ServerUiSettings::load(&state.db).await;
    let visible = boards::list_all(&state.db).await.unwrap_or_default();
    let is_admin = session.as_ref().is_some_and(AuthSession::is_admin);
    let spotlight = build_spotlight_items(state, &visible, is_admin, false).await;
    let mut ctx = PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible,
        spotlight_items: &spotlight,
        ..PageContext::minimal(&meta.name, &body, &locale, &web.i18n)
    };
    ui.apply(&mut ctx);
    Html(render_page(ctx)).into_response()
}
