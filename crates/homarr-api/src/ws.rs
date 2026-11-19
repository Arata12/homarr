use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{
        header::{COOKIE, ORIGIN},
        HeaderMap, StatusCode,
    },
    middleware::Next,
    response::IntoResponse,
};
use homarr_auth::{
    can_view_board, get_session_from_token, session_cookie_name, AuthSession, API_KEY_HEADER_NAME,
};
use homarr_db::repos::boards;
use homarr_redis::RedisStore;
use serde_json::json;

use crate::{
    csrf::{has_session_cookie, has_valid_same_origin},
    state::AppState,
};

/// Middleware rejecting WebSocket requests carrying an `ApiKey` header BEFORE upgrade extraction.
pub async fn reject_api_key_middleware(req: Request, next: Next) -> impl IntoResponse {
    if req.headers().contains_key(API_KEY_HEADER_NAME) {
        return (
            StatusCode::FORBIDDEN,
            "API key authentication is not permitted on WebSocket endpoints",
        )
            .into_response();
    }
    next.run(req).await
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // API keys must NEVER authenticate WebSocket connections (matches Homarr security model).
    if headers.contains_key(API_KEY_HEADER_NAME) {
        return (
            StatusCode::FORBIDDEN,
            "API key authentication is not permitted on WebSocket endpoints",
        )
            .into_response();
    }

    let cookie_name = session_cookie_name(&state.config.auth_cookie_prefix);
    let has_cookie = has_session_cookie(&headers, &cookie_name);
    let has_proxy_auth = state
        .config
        .auth_trusted_proxy_header
        .as_deref()
        .is_some_and(|h| headers.contains_key(h));

    if (headers.contains_key(ORIGIN) || has_cookie || has_proxy_auth)
        && !has_valid_same_origin(&headers, state.config.base_url.as_deref())
    {
        return (
            StatusCode::FORBIDDEN,
            "Cross-origin WebSocket connection rejected",
        )
            .into_response();
    }

    let session_token = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|hdr| {
            hdr.split(';').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                if k.trim() == cookie_name {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
        });

    let session = match session_token {
        Some(tok) => get_session_from_token(&state.db, &tok).await.ok().flatten(),
        None => None,
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, session))
        .into_response()
}

/// Whether a Redis broadcast may be forwarded to this client. Mirrors the original's
/// per-subscription checks: ping results need a signed-in user, board events need view
/// access to that board (signed-in if the event has no board id), everything else
/// (cron job status, unknown channels) is admin-only.
async fn may_receive(
    state: &AppState,
    session: Option<&AuthSession>,
    channel: &str,
    payload: &serde_json::Value,
) -> bool {
    match channel {
        RedisStore::PING_EVENTS_CHANNEL => session.is_some(),
        RedisStore::BOARD_EVENTS_CHANNEL => {
            let Some(board_id) = payload.get("boardId").and_then(|v| v.as_str()) else {
                return session.is_some();
            };
            match boards::get_by_id(&state.db, board_id).await {
                Ok(Some(board)) => can_view_board(&state.db, session, &board)
                    .await
                    .unwrap_or(false),
                _ => false,
            }
        }
        _ => session.is_some_and(AuthSession::is_admin),
    }
}

async fn handle_socket(mut socket: WebSocket, state: AppState, session: Option<AuthSession>) {
    let is_admin = session.as_ref().is_some_and(AuthSession::is_admin);
    let mut redis_rx = state.redis.subscribe();
    let mut logs_rx = state.logs.subscribe();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));

    let welcome = json!({
        "channel": "system:hello",
        "payload": {
            "version": env!("CARGO_PKG_VERSION"),
            "admin": is_admin
        }
    });
    if socket
        .send(Message::Text(welcome.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(p))) => {
                        if socket.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Text(txt))) => {
                        if txt.trim() == "ping" {
                            let _ = socket.send(Message::Text(json!({ "channel": "pong" }).to_string().into())).await;
                        }
                    }
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
            pubsub = redis_rx.recv() => {
                if let Ok(env) = pubsub {
                    let payload_val = match serde_json::from_str::<serde_json::Value>(&env.payload) {
                        Ok(v) => v,
                        Err(_) => serde_json::Value::String(env.payload),
                    };
                    if !may_receive(&state, session.as_ref(), &env.channel, &payload_val).await {
                        continue;
                    }
                    let frame = json!({
                        "channel": env.channel,
                        "payload": payload_val
                    });
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
            log_ev = logs_rx.recv(), if is_admin => {
                if let Ok(entry) = log_ev {
                    let frame = json!({
                        "channel": "system:logs",
                        "payload": entry
                    });
                    if socket.send(Message::Text(frame.to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
            _ = ping_interval.tick() => {
                if socket.send(Message::Ping(vec![1, 2, 3].into())).await.is_err() {
                    break;
                }
            }
        }
    }
}
