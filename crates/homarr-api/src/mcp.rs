use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    Json,
};
use homarr_auth::{api_key::OAuthKeyMeta, create_api_key, AuthSession, RequireAuth};
use homarr_common::{create_id, HomarrError, HomarrResult};
use homarr_db::repos::{
    apps::{self, AppInput},
    boards, integrations, users,
};
use homarr_definitions::GroupPermissionKey;
use homarr_widgets::all_widget_metadata;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

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

#[derive(Deserialize)]
pub struct OAuthRegisterBody {
    pub client_name: Option<String>,
    pub redirect_uris: Option<Vec<String>>,
}

pub async fn oauth_register(
    State(state): State<AppState>,
    Json(body): Json<OAuthRegisterBody>,
) -> HomarrResult<(StatusCode, Json<Value>)> {
    let client_id = create_id();
    let meta = json!({
        "client_id": client_id,
        "client_name": body.client_name.unwrap_or_else(|| "MCP Client".to_string()),
        "redirect_uris": body.redirect_uris.unwrap_or_default(),
    });
    let _ = state
        .redis
        .set_json_ex(&format!("mcp:oauth:client:{client_id}"), &meta, 3600)
        .await;
    Ok((StatusCode::CREATED, Json(meta)))
}

#[derive(Deserialize)]
pub struct OAuthAuthorizeQuery {
    pub client_id: String,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub approve: Option<String>,
}

pub async fn oauth_authorize(
    homarr_auth::OptionalAuth(session): homarr_auth::OptionalAuth,
    State(state): State<AppState>,
    Query(q): Query<OAuthAuthorizeQuery>,
) -> impl IntoResponse {
    let Some(sess) = session else {
        return Redirect::temporary("/auth/login").into_response();
    };
    if q.approve.as_deref() == Some("1") {
        let code = create_id();
        let code_payload = json!({
            "user_id": sess.user_id,
            "client_id": q.client_id,
            "redirect_uri": q.redirect_uri,
        });
        let _ = state
            .redis
            .set_json_ex(&format!("mcp:oauth:code:{code}"), &code_payload, 300)
            .await;
        let sep = if q.redirect_uri.contains('?') {
            '&'
        } else {
            '?'
        };
        let st_part = q
            .state
            .as_deref()
            .map(|s| format!("&state={s}"))
            .unwrap_or_default();
        return Redirect::temporary(&format!("{}{sep}code={code}{st_part}", q.redirect_uri))
            .into_response();
    }
    Html(format!(
        r#"<!doctype html><html><body style="font-family:sans-serif;background:#1a1b1e;color:#fff;padding:2rem">
<h2>Authorize MCP Access</h2>
<p>Client <code>{}</code> is requesting access to your Homarr instance.</p>
<a href="?client_id={}&redirect_uri={}&state={}&approve=1" style="padding:.6rem 1.2rem;background:#fa5252;color:#fff;text-decoration:none;border-radius:6px">Approve Access</a>
</body></html>"#,
        html_escape::encode_text(&q.client_id),
        url::form_urlencoded::byte_serialize(q.client_id.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(q.redirect_uri.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(q.state.unwrap_or_default().as_bytes()).collect::<String>(),
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct OAuthTokenBody {
    pub code: String,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
}

pub async fn oauth_token(
    State(state): State<AppState>,
    axum::Form(body): axum::Form<OAuthTokenBody>,
) -> HomarrResult<Json<Value>> {
    let payload = state
        .redis
        .get_json::<Value>(&format!("mcp:oauth:code:{}", body.code))
        .await?
        .ok_or_else(|| HomarrError::Unauthorized("Invalid or expired authorization code".into()))?;
    let _ = state
        .redis
        .delete(&format!("mcp:oauth:code:{}", body.code))
        .await;

    let user_id = payload
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| HomarrError::Internal("Missing user_id in code".into()))?;
    let client_id = body
        .client_id
        .or_else(|| {
            payload
                .get("client_id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "mcp-client".to_string());
    let redirect_uri = body
        .redirect_uri
        .or_else(|| {
            payload
                .get("redirect_uri")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();

    let (token, _rec) = create_api_key(
        &state.db,
        user_id,
        false,
        Some(OAuthKeyMeta {
            client_id,
            client_name: "MCP OAuth Client".to_string(),
            redirect_uri,
        }),
    )
    .await?;

    Ok(Json(json!({
        "access_token": token,
        "token_type": "Bearer",
        "expires_in": 31_536_000
    })))
}
