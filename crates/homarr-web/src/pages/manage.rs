use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use homarr_auth::{AuthSession, OptionalAuth};
use homarr_db::repos::{
    api_keys, apps, assistant, boards, custom_widgets, groups, integrations, invites, settings,
    users,
};
use homarr_definitions::{integration::all_integration_defs, WidgetKind};
use homarr_widgets::{all_widget_metadata, get_widget_metadata, render_widget_html};
use html_escape::encode_text;
use serde_json::json;

use crate::icons;
use crate::layout::{render_page, PageContext};
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

async fn render_manage_shell(
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

    let mut sidebar = String::from(r#"<aside class="sidebar"><nav class="sidebar-nav">"#);
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
        r#"<div class="manage-shell">{sidebar}<div class="manage-content">{content}</div></div>"#
    );

    Html(render_page(PageContext {
        session,
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible_boards,
        ..PageContext::minimal(title, &full_body, &locale, &web.i18n)
    }))
}

fn require_admin_or_redirect(session: Option<&AuthSession>) -> Result<&AuthSession, Redirect> {
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
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
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
    let users_cnt = users::count(&state.db).await.unwrap_or(0);

    let content = format!(
        r#"<div class="page-header">
  <div>
    <h1 class="page-title">Manage Homarr</h1>
    <p class="muted" style="margin:4px 0 0">Workspace telemetry, boards, apps, integrations, and security settings.</p>
  </div>
</div>
<div class="cards-grid">
  <a class="card kpi-card" href="/manage/boards">
    <div class="kpi-icon kpi-red">{dash_icon}</div>
    <div><div class="muted">Boards</div><div class="metric-big">{boards_cnt}</div></div>
  </a>
  <a class="card kpi-card" href="/manage/apps">
    <div class="kpi-icon kpi-blue">{apps_icon}</div>
    <div><div class="muted">Applications</div><div class="metric-big">{apps_cnt}</div></div>
  </a>
  <a class="card kpi-card" href="/manage/integrations">
    <div class="kpi-icon kpi-green">{plug_icon}</div>
    <div><div class="muted">Integrations</div><div class="metric-big">{ints_cnt}</div></div>
  </a>
  <a class="card kpi-card" href="/manage/users">
    <div class="kpi-icon kpi-yellow">{users_icon}</div>
    <div><div class="muted">Users</div><div class="metric-big">{users_cnt}</div></div>
  </a>
</div>
<div class="card">
  <h3 style="margin-top:0">Quick Actions</h3>
  <div style="display:flex;gap:10px;flex-wrap:wrap">
    <a class="btn btn-primary" href="/manage/boards">{plus_icon}<span>Create Board</span></a>
    <a class="btn" href="/manage/apps">{apps_icon}<span>Add App</span></a>
    <a class="btn" href="/manage/integrations">{plug_icon}<span>Add Integration</span></a>
    <a class="btn" href="/manage/tools/tasks">{clock_icon}<span>Cron Tasks</span></a>
    <a class="btn" href="/manage/assistant">{sparkles_icon}<span>AI Assistant</span></a>
  </div>
</div>"#,
        dash_icon = icons::LAYOUT_DASHBOARD,
        apps_icon = icons::APPS,
        plug_icon = icons::PLUG,
        users_icon = icons::USERS,
        plus_icon = icons::PLUS,
        clock_icon = icons::CLOCK,
        sparkles_icon = icons::SPARKLES,
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
        let id = encode_text(&a.id);
        let href = encode_text(a.href.as_deref().unwrap_or("-"));
        rows.push_str(&format!(
            r#"<tr>
  <td><strong>{name}</strong></td>
  <td><code>{href}</code></td>
  <td><button type="button" class="btn btn-danger btn-sm" data-api-action="/api/apps/{id}" data-method="DELETE">Delete</button></td>
</tr>"#
        ));
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Applications</h1></div>
<div class="card">
  <h3 style="margin-top:0">Add Application</h3>
  <form class="form-grid" data-api-form="/api/apps">
    <div class="form-row"><label>Name</label><input name="name" required placeholder="Sonarr" /></div>
    <div class="form-row"><label>Icon URL</label><input name="iconUrl" required value="/logo/homarr.svg" /></div>
    <div class="form-row"><label>URL</label><input name="href" placeholder="https://sonarr.local" /></div>
    <div class="form-row"><label>Description</label><input name="description" /></div>
    <button type="submit" class="btn btn-primary">Add Application</button>
  </form>
</div>
<div class="card table-wrap">
  <table><thead><tr><th>Name</th><th>URL</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table>
</div>"#
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
            encode_text(def.kind_str),
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
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Integrations</h1></div>
<div class="card">
  <h3 style="margin-top:0">Connect Integration</h3>
  <form class="form-grid" data-api-form="/api/integrations">
    <div class="form-row"><label>Kind (92 Supported)</label><select name="kind">{kinds_opts}</select></div>
    <div class="form-row"><label>Name</label><input name="name" required placeholder="My Sonarr" /></div>
    <div class="form-row"><label>URL</label><input name="url" required placeholder="http://sonarr:8989" /></div>
    <div class="form-row"><label>Secrets JSON (optional)</label><input name="secretsJson" placeholder='[{{"kind":"apiKey","value":"..."}}]' /></div>
    <button type="submit" class="btn btn-primary">Save Integration</button>
  </form>
</div>
<div class="card table-wrap">
  <table><thead><tr><th>Name</th><th>Kind</th><th>URL</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table>
</div>"#
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
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Users</h1></div>
<div class="card">
  <h3 style="margin-top:0">Create User</h3>
  <form class="form-grid" data-api-form="/api/users">
    <div class="form-row"><label>Username</label><input name="name" required minlength="2" /></div>
    <div class="form-row"><label>Email</label><input name="email" type="email" /></div>
    <div class="form-row"><label>Password</label><input name="password" type="password" required minlength="4" /></div>
    <button type="submit" class="btn btn-primary">Create User</button>
  </form>
</div>
<div class="card table-wrap"><table><thead><tr><th>Username</th><th>Email</th><th>Provider</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>"#
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

pub async fn settings_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(redir) = require_admin_or_redirect(session.as_ref()) {
        return redir.into_response();
    }
    let current = settings::get_all_server_settings(&web.app.db)
        .await
        .unwrap_or_default();
    let pretty = serde_json::to_string_pretty(&current).unwrap_or_default();
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Server Settings</h1></div>
<div class="card"><pre class="generic-json">{}</pre></div>"#,
        encode_text(&pretty)
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
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Docker Containers</h1></div>
<div class="card table-wrap"><table><thead><tr><th>Name</th><th>Image</th><th>State</th><th>Actions</th></tr></thead><tbody>{rows}</tbody></table></div>"#
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
        let expr = encode_text(&j.cron_expression);
        let desc = encode_text(&j.description);
        let enabled = if j.is_enabled { "Enabled" } else { "Paused" };
        let btn = if !j.prevent_manual_execution {
            format!(
                r#"<button type="button" class="btn btn-sm" data-api-action="/api/cron-jobs/{name}/trigger" data-method="POST">Run Now</button>"#
            )
        } else {
            String::new()
        };
        rows.push_str(&format!(
            r#"<tr><td><strong>{name}</strong><div class="muted" style="font-size:.78rem">{desc}</div></td><td><code>{expr}</code></td><td><span class="badge">{enabled}</span></td><td>{btn}</td></tr>"#
        ));
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Background Cron Tasks</h1></div>
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
    let mut rows = String::new();
    for l in &entries {
        rows.push_str(&format!(
            r#"<tr><td><code>{}</code></td><td><span class="badge">{}</span></td><td>{}</td></tr>"#,
            encode_text(&l.timestamp),
            encode_text(&l.level),
            encode_text(&l.message),
        ));
    }
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">Server Logs</h1></div>
<div class="card table-wrap"><table><thead><tr><th>Time</th><th>Level</th><th>Message</th></tr></thead><tbody>{rows}</tbody></table></div>"#
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
    let content = format!(
        r#"<div class="page-header"><h1 class="page-title">API Keys</h1></div>
<div class="card">
  <form class="form-grid" data-api-form="/api/api-keys">
    <label style="display:flex;align-items:center;gap:8px"><input type="checkbox" name="readOnly" style="width:auto" /> Read-Only Key</label>
    <button type="submit" class="btn btn-primary">Create API Key</button>
  </form>
</div>
<div class="card table-wrap"><table><thead><tr><th>Key ID</th><th>Scope</th><th>Action</th></tr></thead><tbody>{rows}</tbody></table></div>"#
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
        r#"<div class="page-header"><h1 class="page-title">About Homarr</h1></div>
<div class="card">
  <p><strong>Version:</strong> v{}</p>
  <p><strong>Engine:</strong> Pure Rust Workspace (Axum + SQLx + Tokio + Bollard)</p>
  <p><strong>OpenAPI 3.1 Spec:</strong> <a href="/api/openapi">/api/openapi</a></p>
  <p><strong>MCP Endpoint:</strong> <code>POST /api/mcp</code></p>
</div>"#,
        env!("CARGO_PKG_VERSION")
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

pub async fn api_reference_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let body = r#"<div class="card">
  <h1 class="page-title">Homarr REST, WebSocket &amp; MCP Reference</h1>
  <p class="muted">Inspect the live OpenAPI 3.1.0 specification at <a href="/api/openapi"><code>/api/openapi</code></a>.</p>
  <ul>
    <li><code>GET /api/health/live</code> &middot; <code>GET /api/health/ready</code></li>
    <li><code>POST /api/auth/login</code> &middot; <code>GET /api/auth/session</code></li>
    <li><code>GET / POST /api/boards</code> &middot; <code>GET / PATCH / DELETE /api/boards/{id}</code></li>
    <li><code>GET / POST /api/apps</code> &middot; <code>GET / PATCH / DELETE /api/apps/{id}</code></li>
    <li><code>GET / POST /api/integrations</code> &middot; <code>POST /api/integrations/{id}/test</code></li>
    <li><code>GET /api/widgets/catalog</code> &middot; <code>GET /api/widgets/render/{kind}</code></li>
    <li><code>POST /api/mcp</code> (JSON-RPC 2.0) &middot; <code>GET /websockets</code></li>
  </ul>
</div>"#;

    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        ..PageContext::minimal("API Reference", body, &locale, &web.i18n)
    }))
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
    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        ..PageContext::minimal(&meta.name, &body, &locale, &web.i18n)
    }))
    .into_response()
}
