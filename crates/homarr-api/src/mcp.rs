use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use homarr_auth::{
    api_key::OAuthKeyMeta, create_api_key, AuthSession, RequireAuth, API_KEY_HEADER_NAME,
};
use homarr_common::{create_id, generate_secure_random_token, HomarrError, HomarrResult};
use homarr_db::repos::{
    apps::{self, AppInput},
    boards, integrations, users,
};
use homarr_definitions::GroupPermissionKey;
use homarr_widgets::all_widget_metadata;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{csrf::has_valid_same_origin, state::AppState};

#[derive(Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

fn rpc_ok(id: Option<Value>, result: Value) -> Json<Value> {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result
    }))
}

fn rpc_err(id: Option<Value>, code: i64, message: &str) -> Json<Value> {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message
        }
    }))
}

pub async fn mcp_endpoint(
    RequireAuth(session): RequireAuth,
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let _ = req.jsonrpc;
    match req.method.as_str() {
        "initialize" => rpc_ok(
            req.id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "homarr-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "notifications/initialized" | "ping" => rpc_ok(req.id, json!({})),
        "tools/list" => rpc_ok(req.id, json!({ "tools": mcp_tools_schema() })),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match execute_mcp_tool(&state, &session, name, args).await {
                Ok(val) => rpc_ok(
                    req.id,
                    json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string()) }],
                        "isError": false
                    }),
                ),
                Err(HomarrError::Forbidden(msg)) => rpc_err(req.id, -32003, &msg),
                Err(HomarrError::Unauthorized(msg)) => rpc_err(req.id, -32001, &msg),
                Err(HomarrError::BadRequest(msg)) => rpc_err(req.id, -32602, &msg),
                Err(err) => rpc_err(req.id, -32000, &err.to_string()),
            }
        }
        other => rpc_err(req.id, -32601, &format!("Method '{other}' not found")),
    }
}

fn mcp_tools_schema() -> Vec<Value> {
    vec![
        json!({
            "name": "homarr_list_boards",
            "description": "List dashboards visible to the authenticated user",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "homarr_get_board",
            "description": "Get full layout, sections, and widgets of a board by ID or name",
            "inputSchema": {
                "type": "object",
                "properties": { "idOrName": { "type": "string" } },
                "required": ["idOrName"]
            }
        }),
        json!({
            "name": "homarr_list_apps",
            "description": "List configured Homarr applications",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "homarr_create_app",
            "description": "Create a new application entry (requires write-enabled key & app-create permission)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "iconUrl": { "type": "string" },
                    "href": { "type": "string" },
                    "description": { "type": "string" }
                },
                "required": ["name", "iconUrl"]
            }
        }),
        json!({
            "name": "homarr_delete_app",
            "description": "Delete an application by ID (requires write-enabled key & app-delete-all permission)",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "homarr_list_integrations",
            "description": "List integrations without exposing encrypted secrets",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "homarr_list_widgets",
            "description": "List all 67 supported Homarr widget kinds and default options",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "homarr_system_info",
            "description": "Get Homarr version, counts, and health summary",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "homarr_trigger_cron_job",
            "description": "Trigger a background cron job (admin + write-enabled key)",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }
        }),
        json!({
            "name": "homarr_docker_containers",
            "description": "List Docker containers on configured hosts (admin only)",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

async fn execute_mcp_tool(
    state: &AppState,
    session: &AuthSession,
    name: &str,
    args: Value,
) -> HomarrResult<Value> {
    match name {
        "homarr_list_boards" => {
            let all = boards::list_all(&state.db).await?;
            let mut visible = Vec::new();
            for b in all {
                if homarr_auth::can_view_board(&state.db, Some(session), &b).await? {
                    visible.push(b);
                }
            }
            Ok(json!({ "items": visible }))
        }
        "homarr_get_board" => {
            let target = args
                .get("idOrName")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HomarrError::BadRequest("idOrName is required".into()))?;
            let full = match boards::get_full_board_by_id(&state.db, target).await? {
                Some(b) => b,
                None => boards::get_full_board_by_name(&state.db, target)
                    .await?
                    .ok_or_else(|| HomarrError::NotFound("Board not found".into()))?,
            };
            if !homarr_auth::can_view_board(&state.db, Some(session), &full.board).await? {
                return Err(HomarrError::Forbidden("Board access denied".into()));
            }
            Ok(serde_json::to_value(full).unwrap_or_default())
        }
        "homarr_list_apps" => {
            let items = apps::list_all(&state.db).await?;
            Ok(json!({ "items": items }))
        }
        "homarr_create_app" => {
            session.require_writable(state.demo_read_only_active())?;
            session.require_permission(GroupPermissionKey::AppCreate)?;
            let app_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HomarrError::BadRequest("name is required".into()))?;
            let icon_url = args
                .get("iconUrl")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HomarrError::BadRequest("iconUrl is required".into()))?;
            let created = apps::create(
                &state.db,
                AppInput {
                    id: None,
                    name: app_name.to_string(),
                    description: args
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    icon_url: icon_url.to_string(),
                    href: args
                        .get("href")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    ping_url: None,
                },
            )
            .await?;
            Ok(serde_json::to_value(created).unwrap_or_default())
        }
        "homarr_delete_app" => {
            session.require_writable(state.demo_read_only_active())?;
            session.require_permission(GroupPermissionKey::AppFullAll)?;
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HomarrError::BadRequest("id is required".into()))?;
            let deleted = apps::delete(&state.db, id).await?;
            Ok(json!({ "deleted": deleted }))
        }
        "homarr_list_integrations" => {
            let items = integrations::list_all(&state.db).await?;
            Ok(json!({ "items": items }))
        }
        "homarr_list_widgets" => Ok(json!({ "items": all_widget_metadata() })),
        "homarr_system_info" => {
            let users_count = users::count(&state.db).await?;
            let boards_count = boards::list_all(&state.db).await?.len();
            let apps_count = apps::list_all(&state.db).await?.len();
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "users": users_count,
                "boards": boards_count,
                "apps": apps_count
            }))
        }
        "homarr_trigger_cron_job" => {
            session.require_writable(state.demo_read_only_active())?;
            session.require_permission(GroupPermissionKey::Admin)?;
            let job_name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HomarrError::BadRequest("name is required".into()))?;
            let st = state.scheduler.trigger_job(job_name).await?;
            Ok(serde_json::to_value(st).unwrap_or_default())
        }
        "homarr_docker_containers" => {
            session.require_permission(GroupPermissionKey::Admin)?;
            let items = state.services.docker.list_containers().await?;
            Ok(json!({ "items": items }))
        }
        other => Err(HomarrError::BadRequest(format!(
            "Unknown MCP tool '{other}'"
        ))),
    }
}

pub async fn oauth_protected_resource(State(state): State<AppState>) -> Json<Value> {
    let base = state
        .config
        .base_url
        .clone()
        .unwrap_or_else(|| format!("http://{}:{}", state.config.host, state.config.port));
    let clean = base.trim_end_matches('/');
    Json(json!({
        "resource": format!("{clean}/api/mcp"),
        "authorization_servers": [clean],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["mcp:read", "mcp:write"]
    }))
}

/// Registered OAuth client (dynamic client registration, public client + PKCE).
#[derive(Serialize, Deserialize)]
struct OAuthClient {
    client_id: String,
    client_name: String,
    redirect_uris: Vec<String>,
}

/// A consent request (pending) or an issued authorization code, bound to one user.
#[derive(Serialize, Deserialize)]
struct OAuthGrant {
    user_id: String,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    code_challenge: String,
    state: Option<String>,
    write_requested: bool,
    read_only: bool,
}

const CLIENT_TTL_SECS: u64 = 3600;
const PENDING_TTL_SECS: u64 = 600;
const CODE_TTL_SECS: u64 = 300;

fn client_key(client_id: &str) -> String {
    format!("mcp:oauth:client:{client_id}")
}

fn pending_key(pending_id: &str) -> String {
    format!("mcp:oauth:pending:{pending_id}")
}

fn code_key(code: &str) -> String {
    format!("mcp:oauth:code:{code}")
}

/// HTTP(S) callback without userinfo, fragment, whitespace, backslashes or control characters.
fn valid_callback(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 2048
        || value.contains('#')
        || value.contains('\\')
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let authority = value
        .split_once("://")
        .and_then(|(_, rest)| rest.split(['/', '?']).next())
        .unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    url::Url::parse(value).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

fn valid_code_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn valid_code_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

/// Returns whether write access was requested; read-only scopes alone never allow write.
fn parse_scope(scope: Option<&str>) -> HomarrResult<bool> {
    let scopes: Vec<&str> = scope
        .unwrap_or("mcp:tools")
        .split(' ')
        .filter(|s| !s.is_empty())
        .collect();
    if scopes.is_empty()
        || scopes.iter().any(|s| {
            !matches!(
                *s,
                "mcp:tools" | "mcp:tools:read" | "mcp:read" | "mcp:write"
            )
        })
    {
        return Err(HomarrError::BadRequest(
            "invalid_scope: supported scopes are mcp:tools and mcp:tools:read".into(),
        ));
    }
    Ok(scopes
        .iter()
        .any(|s| matches!(*s, "mcp:tools" | "mcp:write")))
}

#[derive(Deserialize)]
pub struct OAuthRegisterBody {
    pub client_name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
}

pub async fn oauth_register(
    State(state): State<AppState>,
    Json(body): Json<OAuthRegisterBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    let redirect_uris = body.redirect_uris.unwrap_or_default();
    if redirect_uris.is_empty()
        || redirect_uris.len() > 10
        || !redirect_uris.iter().all(|u| valid_callback(u))
    {
        return Err(HomarrError::BadRequest(
            "invalid_client_metadata: redirect_uris must contain 1-10 HTTP(S) callbacks without userinfo or fragments".into(),
        ));
    }
    if (1..redirect_uris.len()).any(|i| redirect_uris[..i].contains(&redirect_uris[i])) {
        return Err(HomarrError::BadRequest(
            "invalid_client_metadata: duplicate redirect_uris".into(),
        ));
    }
    let client_name = body.client_name.unwrap_or_else(|| "MCP Client".to_string());
    if client_name.trim().is_empty()
        || client_name.len() > 256
        || client_name.chars().any(char::is_control)
    {
        return Err(HomarrError::BadRequest(
            "invalid_client_metadata: invalid client_name".into(),
        ));
    }
    let client = OAuthClient {
        client_id: create_id(),
        client_name,
        redirect_uris,
    };
    state
        .redis
        .set_json_ex(&client_key(&client.client_id), &client, CLIENT_TTL_SECS)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "client_id": client.client_id,
            "client_name": client.client_name,
            "redirect_uris": client.redirect_uris,
            "token_endpoint_auth_method": "none",
        })),
    ))
}

#[derive(Deserialize)]
pub struct OAuthAuthorizeQuery {
    pub response_type: Option<String>,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
}

/// GET: validates the request and renders a consent page. It never issues a code.
pub async fn oauth_authorize(
    homarr_auth::OptionalAuth(session): homarr_auth::OptionalAuth,
    State(state): State<AppState>,
    Query(q): Query<OAuthAuthorizeQuery>,
) -> HomarrResult<Response> {
    let code_challenge = q.code_challenge.unwrap_or_default();
    if q.response_type.as_deref() != Some("code")
        || q.code_challenge_method.as_deref() != Some("S256")
        || !valid_code_challenge(&code_challenge)
        || q.state.as_ref().is_some_and(|s| s.len() > 2048)
    {
        return Err(HomarrError::BadRequest(
            "invalid_request: expected response_type=code, a registered callback and explicit S256 PKCE".into(),
        ));
    }
    let write_requested = parse_scope(q.scope.as_deref())?;
    let client = state
        .redis
        .get_json::<OAuthClient>(&client_key(&q.client_id))
        .await?
        .ok_or_else(|| {
            HomarrError::BadRequest(
                "invalid_client: client registration expired or is unavailable; please re-register"
                    .into(),
            )
        })?;
    if !client.redirect_uris.contains(&q.redirect_uri) {
        return Err(HomarrError::BadRequest(
            "invalid_request: redirect_uri not registered".into(),
        ));
    }
    let Some(sess) = session else {
        return Ok(Redirect::temporary("/auth/login").into_response());
    };

    let pending_id = generate_secure_random_token(32);
    let grant = OAuthGrant {
        user_id: sess.user_id,
        client_id: client.client_id,
        client_name: client.client_name,
        redirect_uri: q.redirect_uri,
        code_challenge,
        state: q.state,
        write_requested,
        read_only: true,
    };
    state
        .redis
        .set_json_ex(&pending_key(&pending_id), &grant, PENDING_TTL_SECS)
        .await?;

    let write_option = if write_requested {
        r#"<p><label><input type="checkbox" name="access_mode" value="readWrite"> Also allow write access (create, update and delete data)</label></p>"#
    } else {
        ""
    };
    let page = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Authorize MCP Access</title></head><body style="font-family:sans-serif;background:#1a1b1e;color:#fff;padding:2rem">
<h2>Authorize MCP Access</h2>
<p><strong>{}</strong> (client <code>{}</code>) is requesting access to your Homarr instance.</p>
<p>After approval you will be redirected to <code>{}</code>.</p>
<form method="post" action="/api/mcp/oauth/authorize">
<input type="hidden" name="pending" value="{}">
<p>Access is read-only unless you allow write access below.</p>
{write_option}
<button type="submit" name="decision" value="approve" style="padding:.6rem 1.2rem;background:#fa5252;color:#fff;border:0;border-radius:6px">Approve</button>
<button type="submit" name="decision" value="deny" style="padding:.6rem 1.2rem;border:0;border-radius:6px">Deny</button>
</form>
</body></html>"#,
        html_escape::encode_text(&grant.client_name),
        html_escape::encode_text(&grant.client_id),
        html_escape::encode_text(&grant.redirect_uri),
        html_escape::encode_double_quoted_attribute(&pending_id),
    );
    Ok((
        [
            (header::X_FRAME_OPTIONS, "DENY"),
            (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Html(page),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct OAuthDecisionBody {
    pub pending: String,
    pub decision: String,
    pub access_mode: Option<String>,
}

/// POST: same-origin, cookie-session consent decision; redirects to the client with code or error.
pub async fn oauth_authorize_decision(
    homarr_auth::OptionalAuth(session): homarr_auth::OptionalAuth,
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Form(body): axum::Form<OAuthDecisionBody>,
) -> HomarrResult<Response> {
    if !headers.contains_key(header::ORIGIN)
        || !has_valid_same_origin(&headers, state.config.base_url.as_deref())
    {
        return Err(HomarrError::Forbidden(
            "A same-origin consent decision is required".into(),
        ));
    }
    // An API key must never be able to mint another (possibly broader) API key.
    if headers.contains_key(API_KEY_HEADER_NAME) || headers.contains_key(header::AUTHORIZATION) {
        return Err(HomarrError::Forbidden(
            "Consent requires a browser session".into(),
        ));
    }
    let approve = match body.decision.as_str() {
        "approve" => true,
        "deny" => false,
        _ => {
            return Err(HomarrError::BadRequest(
                "invalid_request: invalid consent decision".into(),
            ))
        }
    };
    let sess =
        session.ok_or_else(|| HomarrError::Unauthorized("Authentication required".into()))?;
    let key = pending_key(&body.pending);
    let mut grant = state
        .redis
        .get_json::<OAuthGrant>(&key)
        .await?
        .filter(|g| g.user_id == sess.user_id)
        .ok_or_else(|| {
            HomarrError::BadRequest("Consent request expired or is unavailable".into())
        })?;
    grant.read_only = match body.access_mode.as_deref() {
        None | Some("readOnly") => true,
        Some("readWrite") if grant.write_requested => false,
        _ => {
            return Err(HomarrError::BadRequest(
                "invalid_request: invalid access mode".into(),
            ))
        }
    };
    state.redis.delete(&key).await?;

    let mut callback = url::Url::parse(&grant.redirect_uri)
        .map_err(|_| HomarrError::Internal("Stored redirect_uri is invalid".into()))?;
    let result = if approve {
        let code = generate_secure_random_token(32);
        state
            .redis
            .set_json_ex(&code_key(&code), &grant, CODE_TTL_SECS)
            .await?;
        ("code", code)
    } else {
        ("error", "access_denied".to_string())
    };
    let kept: Vec<(String, String)> = callback
        .query_pairs()
        .into_owned()
        .filter(|(k, _)| !matches!(k.as_str(), "code" | "error" | "error_description" | "state"))
        .collect();
    {
        let mut query = callback.query_pairs_mut();
        query
            .clear()
            .extend_pairs(kept)
            .append_pair(result.0, &result.1);
        if let Some(st) = &grant.state {
            query.append_pair("state", st);
        }
    }
    Ok(Redirect::to(callback.as_str()).into_response())
}

#[derive(Deserialize)]
pub struct OAuthTokenBody {
    pub grant_type: Option<String>,
    pub code: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_verifier: String,
}

pub async fn oauth_token(
    State(state): State<AppState>,
    axum::Form(body): axum::Form<OAuthTokenBody>,
) -> HomarrResult<Json<Value>> {
    if body
        .grant_type
        .as_deref()
        .is_some_and(|g| g != "authorization_code")
    {
        return Err(HomarrError::BadRequest(
            "unsupported_grant_type: only authorization_code is supported".into(),
        ));
    }
    let key = code_key(&body.code);
    let grant = state
        .redis
        .get_json::<OAuthGrant>(&key)
        .await?
        .ok_or_else(|| HomarrError::Unauthorized("Invalid or expired authorization code".into()))?;
    // Single use: consumed before verification so a failed attempt burns the code.
    state.redis.delete(&key).await?;

    if body.client_id != grant.client_id || body.redirect_uri != grant.redirect_uri {
        return Err(HomarrError::BadRequest(
            "invalid_grant: client or callback mismatch".into(),
        ));
    }
    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(body.code_verifier.as_bytes()));
    if !valid_code_verifier(&body.code_verifier) || computed != grant.code_challenge {
        return Err(HomarrError::BadRequest(
            "invalid_grant: PKCE verification failed".into(),
        ));
    }

    let scope = if grant.read_only {
        "mcp:tools:read"
    } else {
        "mcp:tools"
    };
    let (token, _rec) = create_api_key(
        &state.db,
        &grant.user_id,
        grant.read_only,
        Some(OAuthKeyMeta {
            client_id: grant.client_id,
            client_name: grant.client_name,
            redirect_uri: grant.redirect_uri,
        }),
    )
    .await?;

    Ok(Json(json!({
        "access_token": token,
        "token_type": "Bearer",
        "scope": scope
    })))
}
