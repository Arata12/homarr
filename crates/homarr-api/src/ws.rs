use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{header::COOKIE, HeaderMap, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use homarr_auth::{get_session_from_token, session_cookie_name, API_KEY_HEADER_NAME};
use serde_json::json;

use crate::state::AppState;

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
    let session_token = headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|hdr| {
            hdr.split(';').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                if k.trim() == cookie_name {
                    Some(v.trim().to_string())
                } else {
                    None
                }
            })
        });

    let is_admin = match session_token {
        Some(tok) => get_session_from_token(&state.db, &tok)
            .await
            .ok()
            .flatten()
            .map(|s| s.is_admin())
            .unwrap_or(false),
        None => false,
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, is_admin))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState, is_admin: bool) {
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
