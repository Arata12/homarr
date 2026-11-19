//! Jackett (indexer manager) via its Torznab API. Port of `jackett/jackett-integration.ts`.

use homarr_common::{HomarrError, HomarrResult};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Url;

use super::util::{send_text, url};
use crate::capabilities::indexer_manager::{Indexer, IndexerId};
use crate::error::{classify_request_error, RequestFailure};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const INDEXER_TEST_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JackettIndexer {
    pub id: String,
    pub title: String,
    pub link: Option<String>,
}

fn torznab_url(
    ctx: &IntegrationContext,
    id: &str,
    action: &str,
    extra: &[(&str, &str)],
) -> HomarrResult<Url> {
    let key = ctx.require_secret("apiKey")?;
    let mut url = url(ctx, "/api/v2.0/indexers", &[])?;
    url.path_segments_mut()
        .map_err(|()| HomarrError::BadRequest("Invalid integration URL".to_string()))?
        .extend([id, "results", "torznab", "api"]);
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("apikey", key);
        pairs.append_pair("t", action);
        for (name, value) in extra {
            pairs.append_pair(name, value);
        }
    }
    Ok(url)
}

fn xml_error(ctx: &IntegrationContext, what: &str, reason: impl std::fmt::Display) -> HomarrError {
    crate::error::parse_error(ctx, what, reason)
}

fn root_name(xml: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(start) | Event::Empty(start)) => {
                return Ok(String::from_utf8_lossy(start.name().as_ref()).to_string());
            }
            Ok(Event::Eof) => return Err("empty XML document".to_string()),
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
    }
}

/// `caps` response of the connection test (`{ caps: {} }`).
pub fn validate_caps(xml: &str) -> Result<(), String> {
    match root_name(xml)?.as_str() {
        "caps" => Ok(()),
        other => Err(format!("expected a <caps> document, got <{other}>")),
    }
}

/// Search response (`{ rss: { channel: [..] } }`); a Torznab `<error>` can use HTTP 200.
pub fn validate_search(xml: &str) -> Result<(), String> {
    let root = root_name(xml)?;
    if root != "rss" {
        return Err(format!("expected an <rss> document, got <{root}>"));
    }
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(start) | Event::Empty(start))
                if start.name().as_ref() == b"channel" =>
            {
                return Ok(());
            }
            Ok(Event::Eof) => return Err("the feed has no <channel>".to_string()),
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
    }
}

/// Parses the configured indexers of a Torznab `t=indexers` response.
pub fn parse_indexers(xml: &str) -> Result<Vec<JackettIndexer>, String> {
    #[derive(PartialEq)]
    enum Capture {
        None,
        Title,
        Link,
    }
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut saw_root = false;
    let mut indexer_depth = None;
    let mut current: Option<(String, bool, Option<String>, Option<String>)> = None;
    let mut capture = Capture::None;
    let mut indexers = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                depth += 1;
                let name = start.name();
                let name = name.as_ref();
                if depth == 1 {
                    if name != b"indexers" {
                        return Err(format!(
                            "expected an <indexers> document, got <{}>",
                            String::from_utf8_lossy(name)
                        ));
                    }
                    saw_root = true;
                } else if depth == 2 && name == b"indexer" {
                    let mut id = None;
                    let mut configured = false;
                    for attribute in start.attributes().flatten() {
                        let value = attribute
                            .unescape_value()
                            .map_err(|err| err.to_string())?
                            .to_string();
                        match attribute.key.as_ref() {
                            b"id" => id = Some(value),
                            b"configured" => configured = value == "true",
                            _ => {}
                        }
                    }
                    let id = id
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| "indexer without an id".to_string())?;
                    current = Some((id, configured, None, None));
                    indexer_depth = Some(depth);
                } else if Some(depth) == indexer_depth.map(|d| d + 1) {
                    capture = match name {
                        b"title" => Capture::Title,
                        b"link" => Capture::Link,
                        _ => Capture::None,
                    };
                }
            }
            Ok(Event::Empty(start)) => {
                if depth == 0 {
                    // `<indexers/>`
                    if start.name().as_ref() != b"indexers" {
                        return Err("expected an <indexers> document".to_string());
                    }
                    saw_root = true;
                }
            }
            Ok(Event::Text(text)) => {
                if let Some((_, _, title, link)) = current.as_mut() {
                    let value = text.unescape().map_err(|err| err.to_string())?.to_string();
                    match capture {
                        Capture::Title if title.is_none() => *title = Some(value),
                        Capture::Link if link.is_none() => *link = Some(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(_)) => {
                capture = Capture::None;
                if Some(depth) == indexer_depth {
                    if let Some((id, configured, title, link)) = current.take() {
                        if configured {
                            indexers.push(JackettIndexer {
                                title: title.unwrap_or_else(|| id.clone()),
                                id,
                                link,
                            });
                        }
                    }
                    indexer_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(err.to_string()),
            _ => {}
        }
    }
    if !saw_root {
        return Err("empty XML document".to_string());
    }
    Ok(indexers)
}

async fn configured_indexers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<JackettIndexer>> {
    let url = torznab_url(ctx, "all", "indexers", &[("configured", "true")])?;
    let xml = send_text(ctx, rt.http.no_redirect().get(url)).await?;
    parse_indexers(&xml).map_err(|err| xml_error(ctx, "the indexers", err))
}

pub async fn indexers(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<Vec<Indexer>> {
    Ok(configured_indexers(rt, ctx)
        .await?
        .into_iter()
        .map(|indexer| Indexer {
            id: IndexerId::Text(indexer.id),
            name: indexer.title,
            url: indexer.link.unwrap_or_default(),
            enabled: true,
            // Jackett's Torznab catalog does not expose health or last-test results.
            status: None,
        })
        .collect())
}

async fn test_indexer(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    indexer: &JackettIndexer,
) -> Result<(), String> {
    let url =
        torznab_url(ctx, &indexer.id, "search", &[]).map_err(|_| "Test failed".to_string())?;
    let response = rt.http.no_redirect().get(url).send().await.map_err(|err| {
        match classify_request_error(&err) {
            RequestFailure::Timeout => "Request timed out",
            RequestFailure::Certificate => "Certificate error",
            RequestFailure::Connection => "Connection failed",
            _ => "Request failed",
        }
        .to_string()
    })?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let xml = response
        .text()
        .await
        .map_err(|_| "Request failed".to_string())?;
    validate_search(&xml).map_err(|_| "Invalid Torznab response".to_string())
}

pub async fn test_all(rt: &IntegrationRuntime, ctx: &IntegrationContext) -> HomarrResult<()> {
    let indexers = configured_indexers(rt, ctx).await?;
    let mut failures = Vec::new();
    for chunk in indexers.chunks(INDEXER_TEST_CONCURRENCY) {
        let results = futures_util::future::join_all(
            chunk.iter().map(|indexer| test_indexer(rt, ctx, indexer)),
        )
        .await;
        for (indexer, result) in chunk.iter().zip(results) {
            if let Err(reason) = result {
                failures.push(format!("{} ({}): {reason}", indexer.title, indexer.id));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(HomarrError::Upstream(format!(
            "{} of {} Jackett indexers failed: {}",
            failures.len(),
            indexers.len(),
            failures.join(", ")
        )))
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let xml = send_text(
        ctx,
        rt.http
            .no_redirect()
            .get(torznab_url(ctx, "all", "caps", &[])?),
    )
    .await?;
    validate_caps(&xml).map_err(|err| xml_error(ctx, "the capabilities", err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_configured_indexers_with_direct_children() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<indexers>
  <indexer id="alpha" configured="true">
    <title>Alpha &amp; Co</title>
    <link>https://alpha.example/</link>
    <caps><server title="x"/><categories><category id="1" name="Movies"/></categories></caps>
  </indexer>
  <indexer id="beta" configured="false"><title>Beta</title></indexer>
  <indexer id="gamma" configured="true"><title>Gamma</title></indexer>
</indexers>"#;
        let parsed = parse_indexers(xml).unwrap();
        assert_eq!(
            parsed,
            vec![
                JackettIndexer {
                    id: "alpha".into(),
                    title: "Alpha & Co".into(),
                    link: Some("https://alpha.example/".into())
                },
                JackettIndexer {
                    id: "gamma".into(),
                    title: "Gamma".into(),
                    link: None
                },
            ]
        );
    }

    #[test]
    fn empty_indexers_and_invalid_documents() {
        assert!(parse_indexers("<indexers/>").unwrap().is_empty());
        assert!(parse_indexers("<indexers></indexers>").unwrap().is_empty());
        assert!(parse_indexers(r#"<error code="100" description="Invalid API Key"/>"#).is_err());
    }

    #[test]
    fn torznab_responses_are_validated() {
        assert!(validate_caps("<caps><server/></caps>").is_ok());
        assert!(validate_caps("<error code=\"100\"/>").is_err());
        assert!(validate_search("<rss><channel><title>x</title></channel></rss>").is_ok());
        assert!(validate_search("<rss></rss>").is_err());
        assert!(validate_search("<error code=\"101\"/>").is_err());
    }
}
