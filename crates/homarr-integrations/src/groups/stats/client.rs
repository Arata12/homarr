//! Request layer of the stats providers (port of `stats-integration.ts`).
//!
//! Provider paths are compiled in, never user input. Kinds whose declared HTTP auth is not an adapter get
//! that auth (headers and query) applied to every request; adapter kinds authenticate through the provider
//! itself, exactly like the TS `StatsIntegration`.

use homarr_common::{HomarrError, HomarrResult};
use homarr_definitions::integration::{get_integration_def, IntegrationHttpAuthDefinition};
use reqwest::header::HeaderMap;
use reqwest::Method;
use serde_json::Value;

use crate::clients::build_request_url;
use crate::error::{check_status, parse_error, request_error};
use crate::http_auth::{resolve_declared_http_auth, ResolvedHttpAuth};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

/// Header name and value pairs. Names are always compiled-in constants.
pub type Headers = Vec<(&'static str, String)>;

pub enum Body {
    None,
    Json(Value),
    Form(Vec<(&'static str, String)>),
}

pub struct StatsClient<'a> {
    rt: &'a IntegrationRuntime,
    pub ctx: &'a IntegrationContext,
    declared: ResolvedHttpAuth,
}

impl<'a> StatsClient<'a> {
    pub fn new(rt: &'a IntegrationRuntime, ctx: &'a IntegrationContext) -> HomarrResult<Self> {
        let adapter = matches!(
            get_integration_def(ctx.kind).http_auth,
            IntegrationHttpAuthDefinition::Adapter
        );
        let declared = if adapter {
            ResolvedHttpAuth::default()
        } else {
            resolve_declared_http_auth(ctx)?
        };
        Ok(Self { rt, ctx, declared })
    }

    /// The saved secret, `None` when absent or empty (TS `hasSecret`).
    pub fn secret(&self, kind: &str) -> Option<&str> {
        self.ctx.secret(kind).filter(|value| !value.is_empty())
    }

    pub fn require_secret(&self, kind: &str) -> HomarrResult<&str> {
        self.secret(kind).ok_or_else(|| {
            HomarrError::BadRequest(format!(
                "Integration credential '{kind}' is missing for {}",
                self.ctx.name
            ))
        })
    }

    pub async fn get(&self, path: &str, headers: &Headers) -> HomarrResult<Value> {
        self.send(Method::GET, path, headers, Body::None).await
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        headers: &Headers,
        body: Body,
    ) -> HomarrResult<reqwest::RequestBuilder> {
        let url = build_request_url(self.ctx.base_url(), path, &self.declared.query)?;
        let mut request = self.rt.http.standard().request(method, url);
        for (name, value) in &self.declared.headers {
            request = request.header(name, value);
        }
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        Ok(match body {
            Body::None => request,
            Body::Json(value) => request.json(&value),
            Body::Form(fields) => request.form(&fields),
        })
    }

    /// Sends a request and parses the JSON response body.
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        headers: &Headers,
        body: Body,
    ) -> HomarrResult<Value> {
        let response = self
            .request(method, path, headers, body)?
            .send()
            .await
            .map_err(|err| request_error(self.ctx, &err))?;
        let response = check_status(self.ctx, response)?;
        response
            .json::<Value>()
            .await
            .map_err(|err| crate::error::response_body_error(self.ctx, "the response", err))
    }

    /// Sends a request whose body is discarded and returns the response headers.
    /// Used for cookie based logins; anything but HTTP 200 fails (TS `requestResponseAsync`).
    pub async fn send_for_headers(
        &self,
        method: Method,
        path: &str,
        headers: &Headers,
        body: Body,
    ) -> HomarrResult<HeaderMap> {
        let response = self
            .request(method, path, headers, body)?
            .send()
            .await
            .map_err(|err| request_error(self.ctx, &err))?;
        if response.status().as_u16() != 200 {
            return Err(crate::error::status_error(self.ctx, response.status()));
        }
        Ok(response.headers().clone())
    }
}

/// Maps a validation failure message of a pure parser to the integration error.
pub fn parsed<T>(
    ctx: &IntegrationContext,
    what: &str,
    result: Result<T, String>,
) -> HomarrResult<T> {
    result.map_err(|message| parse_error(ctx, what, message))
}
