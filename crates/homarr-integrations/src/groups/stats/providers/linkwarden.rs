use std::collections::HashSet;

use async_trait::async_trait;
use homarr_common::{HomarrError, HomarrResult};
use serde_json::Value;

use super::{m, values, MetricDef, StatsProvider};
use crate::capabilities::stats::{StatsUnit::Count, StatsValues};
use crate::groups::stats::client::{parsed, StatsClient};
use crate::groups::stats::json::{array, count, len, num};

pub struct Linkwarden;

const METRICS: &[MetricDef] = &[
    m("links", "Links", Count),
    m("collections", "Collections", Count),
    m("tags", "Tags", Count),
];

const MAX_TAG_PAGES: usize = 1000;

#[async_trait]
impl StatsProvider for Linkwarden {
    fn metrics(&self) -> &'static [MetricDef] {
        METRICS
    }

    async fn fetch(&self, client: &StatsClient<'_>) -> HomarrResult<StatsValues> {
        let none = Vec::new();
        let (collections, tags) = tokio::try_join!(
            client.get("/api/v1/collections", &none),
            client.get("/api/v1/tags", &none),
        )?;
        let (links, collection_count) = parsed(
            client.ctx,
            "Linkwarden collections",
            collection_stats(&collections),
        )?;
        let mut page = parsed(client.ctx, "Linkwarden tags", tag_page(&tags))?;
        let mut tag_count = page.tags;
        let mut visited = HashSet::new();
        while let Some(cursor) = page.next_cursor {
            if visited.contains(&cursor) || visited.len() >= MAX_TAG_PAGES {
                return Err(HomarrError::Upstream(
                    "Invalid or excessive Linkwarden tag pagination".to_string(),
                ));
            }
            let path = format!("/api/v1/tags?cursor={}", urlencode(&cursor));
            visited.insert(cursor);
            let response = client.get(&path, &none).await?;
            page = parsed(client.ctx, "Linkwarden tags", tag_page(&response))?;
            tag_count += page.tags;
        }
        Ok(values([
            ("links", num(links)),
            ("collections", len(collection_count)),
            ("tags", len(tag_count)),
        ]))
    }
}

fn urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Total links and number of collections; accepts the three envelopes Linkwarden versions return.
fn collection_stats(value: &Value) -> Result<(f64, usize), String> {
    let list = if let Some(list) = value.as_array() {
        list
    } else if value.get("response").is_some() {
        array(value, &["response"])?
    } else {
        array(value, &["data", "collections"])?
    };
    let mut links = 0.0;
    for collection in list {
        links += count(collection, &["_count", "links"])?;
    }
    Ok((links, list.len()))
}

struct TagPage {
    tags: usize,
    next_cursor: Option<String>,
}

fn tag_page(value: &Value) -> Result<TagPage, String> {
    // Prefer the paginated envelope when v1 also includes a legacy response field.
    if let Some(data) = value.get("data").filter(|data| data.get("tags").is_some()) {
        let tags = array(data, &["tags"])?.len();
        let next_cursor = match data.get("nextCursor") {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(Value::Number(number)) => {
                crate::groups::stats::json::as_count(&Value::Number(number.clone()), "nextCursor")?;
                Some(number.to_string())
            }
            Some(_) => return Err("'nextCursor' must be a number or string".to_string()),
        };
        return Ok(TagPage { tags, next_cursor });
    }
    let tags = if let Some(list) = value.as_array() {
        list.len()
    } else {
        array(value, &["response"])?.len()
    };
    Ok(TagPage {
        tags,
        next_cursor: None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn collection_envelopes() {
        let c = json!([{ "_count": { "links": 2 } }, { "_count": { "links": 3 } }]);
        assert_eq!(collection_stats(&c), Ok((5.0, 2)));
        assert_eq!(collection_stats(&json!({ "response": c })), Ok((5.0, 2)));
        assert_eq!(
            collection_stats(&json!({ "data": { "collections": c } })),
            Ok((5.0, 2))
        );
    }

    #[test]
    fn tag_pages() {
        let page =
            tag_page(&json!({ "data": { "tags": [{}, {}], "nextCursor": 7 }, "response": [] }))
                .unwrap();
        assert_eq!((page.tags, page.next_cursor.as_deref()), (2, Some("7")));
        let page = tag_page(&json!({ "response": [{}] })).unwrap();
        assert_eq!((page.tags, page.next_cursor), (1, None));
        let page = tag_page(&json!([{}, {}, {}])).unwrap();
        assert_eq!(page.tags, 3);
        assert!(tag_page(&json!({ "data": { "tags": [], "nextCursor": 1.5 } })).is_err());
    }
}
