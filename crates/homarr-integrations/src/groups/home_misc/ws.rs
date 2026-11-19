//! Minimal websocket client helper (Home Assistant, ArchiveTeam Warrior).
//!
//! Uses `IntegrationRuntime::ws_tls_config()` so custom certificates managed by
//! `CertificateManager` are honored alongside `webpki-roots`.

use std::time::Duration;

use homarr_common::{HomarrError, HomarrResult};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};
use url::Url;

use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WS_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_WS_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// `http(s)://` -> `ws(s)://`.
pub fn to_ws_url(mut url: Url) -> HomarrResult<Url> {
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|()| HomarrError::BadRequest("Invalid websocket URL".into()))?;
    Ok(url)
}

pub async fn connect(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    url: &Url,
    headers: &[(&str, String)],
) -> HomarrResult<Socket> {
    let mut request = url.as_str().into_client_request().map_err(|err| {
        HomarrError::BadRequest(format!("Invalid websocket URL for {}: {err}", ctx.name))
    })?;
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|err| HomarrError::BadRequest(format!("Invalid header name: {err}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|err| HomarrError::BadRequest(format!("Invalid header value: {err}")))?;
        request.headers_mut().insert(name, value);
    }
    let connector = if url.scheme() == "wss" {
        Some(Connector::Rustls(rt.ws_tls_config()?))
    } else {
        None
    };
    let mut cfg = WebSocketConfig::default();
    cfg.max_message_size = Some(MAX_WS_MESSAGE_BYTES);
    cfg.max_frame_size = Some(MAX_WS_FRAME_BYTES);
    match tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect_async_tls_with_config(request, Some(cfg), false, connector),
    )
    .await
    {
        Err(_) => Err(HomarrError::Upstream(format!(
            "Connecting to {} timed out",
            ctx.name
        ))),
        Ok(Err(err)) => Err(HomarrError::Upstream(format!(
            "Unable to open a websocket connection to {}: {}",
            ctx.name,
            describe(&err)
        ))),
        Ok(Ok((socket, _))) => Ok(socket),
    }
}

fn describe(err: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match err {
        Error::Http(response) => format!("HTTP {}", response.status().as_u16()),
        Error::Tls(_) => "the TLS certificate could not be verified".to_string(),
        other => other.to_string(),
    }
}
