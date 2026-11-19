use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::Html,
};
use homarr_auth::OptionalAuth;
use homarr_db::repos::{boards, users};
use html_escape::encode_text;

use crate::icons;
use crate::layout::{render_page, PageContext};
use crate::WebState;

fn wrap_auth_shell(heading: &str, subtitle: &str, inner: &str) -> String {
    format!(
        r#"<div class="auth-shell">
  <div class="auth-floating-bar">
    <a class="icon-btn" href="/" title="Back to Home Board" aria-label="Back to Home Board">{home}</a>
    <button type="button" class="icon-btn" onclick="window.toggleTheme()" title="Toggle theme" aria-label="Toggle theme">{theme}</button>
  </div>
  <div class="auth-card">
    <div class="auth-brand">
      <img src="/logo/homarr.svg" alt="Homarr" class="auth-logo" />
      <h1 class="auth-title">{heading}</h1>
      <p class="auth-subtitle">{subtitle}</p>
    </div>
    {inner}
  </div>
</div>"#,
        home = icons::HOME,
        theme = icons::SUN_MOON,
        heading = encode_text(heading),
        subtitle = encode_text(subtitle),
    )
}

pub async fn login_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let visible_boards = boards::list_all(&state.db).await.unwrap_or_default();

    let demo_hint = if state.config.demo_mode {
        r#"<div class="auth-demo-alert">
  <div>
    <strong>Demo Mode Active</strong>
    <div class="muted" style="font-size:.82rem;margin-top:2px">Username: <code>demo</code> &middot; Password: <code>demo</code></div>
  </div>
  <button type="button" class="btn btn-sm" onclick="window.fillDemoLogin()">Fill</button>
</div>"#
    } else {
        ""
    };

    let inner = format!(
        r#"{demo_hint}
<form id="login-form" class="form-grid" style="max-width:none" data-api-form="/api/auth/login" data-redirect="/">
  <div class="form-row">
    <label for="name">Username</label>
    <input id="name" name="name" placeholder="Enter your username" required autofocus />
  </div>
  <div class="form-row">
    <label for="password">Password</label>
    <input id="password" name="password" type="password" placeholder="••••••••" required />
  </div>
  <button type="submit" class="btn btn-primary btn-full" style="margin-top:4px">{user_icon}<span>Sign In</span></button>
</form>"#,
        user_icon = icons::USER,
    );

    let body = wrap_auth_shell(
        "Welcome back",
        "Sign in to access your Homarr dashboard.",
        &inner,
    );

    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        boards: &visible_boards,
        background_image_url: Some("/images/demo-dashboard-background.svg"),
        opacity: Some(88),
        hide_header: true,
        ..PageContext::minimal("Sign In", &body, &locale, &web.i18n)
    }))
}

pub async fn invite_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let safe_token = encode_text(&token);

    let inner = format!(
        r#"<form class="form-grid" style="max-width:none" data-api-form="/api/auth/register" data-redirect="/">
  <input type="hidden" name="inviteToken" value="{safe_token}" />
  <div class="form-row">
    <label for="name">Username</label>
    <input id="name" name="name" required minlength="2" placeholder="Choose a username" />
  </div>
  <div class="form-row">
    <label for="email">Email (optional)</label>
    <input id="email" name="email" type="email" placeholder="you@homelab.local" />
  </div>
  <div class="form-row">
    <label for="password">Password</label>
    <input id="password" name="password" type="password" required minlength="8" placeholder="At least 8 characters" />
  </div>
  <button type="submit" class="btn btn-primary btn-full">Create Account</button>
</form>"#
    );

    let body = wrap_auth_shell(
        "Accept Invitation",
        "Create your Homarr account using the invitation link.",
        &inner,
    );

    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        background_image_url: Some("/images/demo-dashboard-background.svg"),
        opacity: Some(88),
        hide_header: true,
        ..PageContext::minimal("Register", &body, &locale, &web.i18n)
    }))
}

pub async fn onboarding_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let user_count = users::count(&state.db).await.unwrap_or(0);
    let welcome = web
        .i18n
        .t(&locale, "init.studio.welcome.title", "Welcome home.");

    let account_step = if user_count == 0 {
        r#"<div class="init-step-box">
  <h3>Create Administrator</h3>
  <p class="muted" style="margin-top:0;font-size:.86rem">Set up your first administrator account to launch Homarr.</p>
  <form class="form-grid" style="max-width:none" data-api-form="/api/auth/register" data-redirect="/">
    <div class="form-row"><label>Username</label><input name="name" required minlength="2" placeholder="admin" /></div>
    <div class="form-row"><label>Email</label><input name="email" type="email" placeholder="admin@homarr.local" /></div>
    <div class="form-row"><label>Password</label><input name="password" type="password" required minlength="8" /></div>
    <button type="submit" class="btn btn-primary btn-full">Create Administrator &amp; Launch</button>
  </form>
</div>"#
    } else {
        r#"<div class="init-step-box">
  <h3>Workspace Ready</h3>
  <p class="muted" style="margin-top:0;font-size:.86rem">Your administrator account is configured.</p>
  <a class="btn btn-primary btn-full" href="/">Open Home Board</a>
</div>"#
    };

    let inner = format!(
        r#"<div style="display:flex;justify-content:center;margin-bottom:14px">
  <span class="demo-pill">Homarr v{version}</span>
</div>
{account_step}
<div class="init-checklist">
  <div class="muted" style="font-size:.78rem;text-transform:uppercase;letter-spacing:.05em;margin-bottom:8px">Quick Setup</div>
  <div style="display:flex;gap:8px;flex-wrap:wrap">
    <a class="btn btn-sm" href="/manage/boards">Boards</a>
    <a class="btn btn-sm" href="/manage/apps">Applications</a>
    <a class="btn btn-sm" href="/manage/integrations">Integrations</a>
    <a class="btn btn-sm" href="/manage/settings">Settings</a>
  </div>
</div>"#,
        version = env!("CARGO_PKG_VERSION"),
    );

    let body = wrap_auth_shell(
        &welcome,
        "Guided onboarding studio for your Homarr workspace.",
        &inner,
    );

    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        background_image_url: Some("/images/demo-dashboard-background.svg"),
        opacity: Some(88),
        hide_header: true,
        ..PageContext::minimal("Onboarding", &body, &locale, &web.i18n)
    }))
}

pub async fn mcp_auth_page(
    OptionalAuth(session): OptionalAuth,
    State(web): State<WebState>,
    headers: HeaderMap,
) -> Html<String> {
    let state = &web.app;
    let locale = web.i18n.negotiate(&headers);
    let body = r#"<div style="max-width:680px;margin:24px auto">
  <div class="card">
    <h1 class="page-title">Homarr MCP Integration</h1>
    <p class="muted">Connect AI agents and MCP clients to <code>/api/mcp</code> via JSON-RPC 2.0.</p>
    <ul>
      <li><strong>Endpoint:</strong> <code>POST /api/mcp</code></li>
      <li><strong>Auth:</strong> <code>ApiKey: &lt;id&gt;.&lt;secret&gt;</code> or <code>Authorization: Bearer &lt;token&gt;</code></li>
      <li><strong>OAuth Protected Resource Metadata:</strong> <code>GET /.well-known/oauth-protected-resource</code></li>
      <li><strong>Tools:</strong> boards, apps, integrations, widgets, system info, cron jobs, Docker containers</li>
    </ul>
    <a class="btn btn-primary" href="/manage/tools/api-keys">Manage API Keys</a>
  </div>
</div>"#;

    Html(render_page(PageContext {
        session: session.as_ref(),
        demo_mode: state.config.demo_mode,
        demo_read_only: state.config.demo_read_only,
        ..PageContext::minimal("MCP Setup", body, &locale, &web.i18n)
    }))
}
