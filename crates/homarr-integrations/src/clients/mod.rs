use std::collections::HashMap;

use homarr_common::{HomarrError, HomarrResult};
use url::Url;

pub(crate) fn build_request_url(
    base: &str,
    path: &str,
    query: &HashMap<String, String>,
) -> HomarrResult<Url> {
    let base_url = Url::parse(base.trim())
        .map_err(|err| HomarrError::BadRequest(format!("Invalid integration base URL: {err}")))?;
    if !matches!(base_url.scheme(), "http" | "https")
        || base_url.host_str().is_none_or(str::is_empty)
    {
        return Err(HomarrError::BadRequest(
            "Integration URL must use http:// or https:// with a valid host".into(),
        ));
    }
    if path.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(HomarrError::BadRequest(
            "Integration request path cannot contain '..' segments".into(),
        ));
    }
    let joined = format!(
        "{}/{}",
        base_url.as_str().trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut url = Url::parse(&joined)
        .map_err(|err| HomarrError::BadRequest(format!("Invalid integration URL: {err}")))?;
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query {
            pairs.append_pair(k, v);
        }
    }
    Ok(url)
}
