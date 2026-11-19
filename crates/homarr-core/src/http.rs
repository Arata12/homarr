use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use homarr_common::{is_blocked_ip, validate_outbound_url, HomarrError, HomarrResult};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::Policy;
use reqwest::{Client, Response};
use url::Url;

use crate::certificates::CertificateManager;

const MAX_REDIRECTS: usize = 5;

/// Resolver that drops blocked addresses at connect time, so a DNS answer cannot change
/// between the pre-flight check and the actual connection (DNS rebinding).
struct BlockedIpFilteringResolver;

impl Resolve for BlockedIpFilteringResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(resolve_unblocked(name.as_str().to_string()))
    }
}

async fn resolve_unblocked(
    host: String,
) -> Result<Addrs, Box<dyn std::error::Error + Send + Sync>> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
        .await?
        .filter(|addr| !is_blocked_ip(addr.ip()))
        .collect();
    if addrs.is_empty() {
        return Err(format!("{host} resolves only to blocked addresses").into());
    }
    Ok(Box::new(addrs.into_iter()))
}

#[derive(Debug, Clone)]
pub struct HttpClientFactory {
    standard: Client,
    no_redirect: Client,
    guarded: Client,
    ping: Client,
    certs: CertificateManager,
}

impl HttpClientFactory {
    pub fn new(certs: &CertificateManager) -> HomarrResult<Self> {
        let extra_certs = certs.load_reqwest_certificates();

        let mut standard_builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("Homarr/", env!("CARGO_PKG_VERSION")));
        for cert in &extra_certs {
            standard_builder = standard_builder.add_root_certificate(cert.clone());
        }
        let standard = standard_builder
            .build()
            .map_err(|err| HomarrError::Internal(format!("Failed to build HTTP client: {err}")))?;

        let mut no_redirect_builder = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .user_agent(concat!("Homarr/", env!("CARGO_PKG_VERSION")));
        let mut guarded_builder = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .dns_resolver(Arc::new(BlockedIpFilteringResolver))
            .user_agent(concat!("Homarr/", env!("CARGO_PKG_VERSION")));
        for cert in extra_certs {
            no_redirect_builder = no_redirect_builder.add_root_certificate(cert.clone());
            guarded_builder = guarded_builder.add_root_certificate(cert);
        }
        let guarded = guarded_builder.build().map_err(|err| {
            HomarrError::Internal(format!("Failed to build guarded HTTP client: {err}"))
        })?;
        let no_redirect = no_redirect_builder.build().map_err(|err| {
            HomarrError::Internal(format!("Failed to build no-redirect HTTP client: {err}"))
        })?;

        let ping = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .user_agent(concat!("Homarr-Ping/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| HomarrError::Internal(format!("Failed to build ping client: {err}")))?;

        Ok(Self {
            standard,
            no_redirect,
            guarded,
            ping,
            certs: certs.clone(),
        })
    }

    pub fn standard(&self) -> &Client {
        &self.standard
    }

    pub fn no_redirect(&self) -> &Client {
        &self.no_redirect
    }

    /// Client for untrusted URLs: no redirects, blocked IPs filtered at connect time.
    pub fn guarded(&self) -> &Client {
        &self.guarded
    }

    pub fn ping(&self) -> &Client {
        &self.ping
    }

    pub fn certs(&self) -> &CertificateManager {
        &self.certs
    }

    /// Requests a URL with SSRF protection (DNS lookup checked against blocked ranges
    /// unless `trusted_origin` matches) and manual redirect following up to 5 hops.
    pub async fn guarded_get(
        &self,
        initial_url: &str,
        initial_headers: Option<&HashMap<String, String>>,
        trusted_origin: Option<&str>,
        max_bytes: usize,
    ) -> HomarrResult<(Vec<u8>, Option<String>, u16)> {
        let mut current_url = validate_outbound_url(initial_url, trusted_origin)?;
        let mut current_headers = initial_headers.cloned();
        let mut current_trusted = trusted_origin.map(str::to_string);

        for hop in 0..=MAX_REDIRECTS {
            let origin = current_url.origin().ascii_serialization();
            let is_trusted = current_trusted.as_deref() == Some(origin.as_str());
            if !is_trusted {
                verify_dns_targets(&current_url).await?;
            }

            let client = if is_trusted {
                &self.no_redirect
            } else {
                &self.guarded
            };
            let mut req = client.get(current_url.clone());
            if let Some(ref headers) = current_headers {
                for (k, v) in headers {
                    req = req.header(k, v);
                }
            }
            let resp = req.send().await.map_err(|err| {
                HomarrError::Upstream(format!("Request to {origin} failed: {err}"))
            })?;

            let status = resp.status();
            if status.is_redirection() {
                if hop == MAX_REDIRECTS {
                    return Err(HomarrError::Upstream(format!(
                        "Too many redirects starting at {}",
                        current_url.origin().ascii_serialization()
                    )));
                }
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        HomarrError::Upstream(
                            "Redirect response missing Location header".to_string(),
                        )
                    })?;
                let mut next = current_url
                    .join(location)
                    .map_err(|_| HomarrError::Upstream("Invalid redirect location".to_string()))?;
                let _ = next.set_username("");
                let _ = next.set_password(None);
                let next_origin = next.origin().ascii_serialization();
                if next_origin != origin {
                    current_headers = None;
                    current_trusted = None;
                }
                current_url = validate_outbound_url(next.as_str(), current_trusted.as_deref())?;
                continue;
            }

            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let status_code = status.as_u16();
            if !status.is_success() {
                return Ok((Vec::new(), content_type, status_code));
            }
            let bytes = read_bounded_bytes(resp, max_bytes).await?;
            return Ok((bytes, content_type, status_code));
        }

        Err(HomarrError::Upstream("Too many redirects".to_string()))
    }
}

async fn verify_dns_targets(url: &Url) -> HomarrResult<()> {
    let host = url
        .host_str()
        .ok_or_else(|| HomarrError::BadRequest("URL missing host".to_string()))?;
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| HomarrError::Upstream(format!("Failed to resolve {host}: {err}")))?;
    let mut found_any = false;
    for addr in addrs {
        found_any = true;
        if is_blocked_ip(addr.ip()) {
            return Err(HomarrError::Forbidden(format!(
                "Refusing to request {} because it resolves to blocked address {}",
                url.origin().ascii_serialization(),
                addr.ip()
            )));
        }
    }
    if !found_any {
        return Err(HomarrError::Upstream(format!(
            "{host} did not resolve to any address"
        )));
    }
    Ok(())
}

pub async fn read_bounded_bytes(response: Response, max_bytes: usize) -> HomarrResult<Vec<u8>> {
    if let Some(len) = response.content_length() {
        if len as usize > max_bytes {
            return Err(HomarrError::BadRequest(format!(
                "Response exceeded maximum allowed size of {max_bytes} bytes"
            )));
        }
    }
    let mut stream = response.bytes_stream();
    let mut buf = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|err| HomarrError::Upstream(format!("Failed reading response body: {err}")))?;
        if buf.len() + chunk.len() > max_bytes {
            return Err(HomarrError::BadRequest(format!(
                "Response exceeded maximum allowed size of {max_bytes} bytes"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}
