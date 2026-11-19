//! Anchor: notes (`anchor-integration.ts`).

use homarr_common::{HomarrError, HomarrResult};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::util::{
    input_str, input_u64, parse_json, send, send_raw, to_json, unknown_operation, url,
};
use crate::error::{check_status, request_error};
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorNoteSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub is_pinned: bool,
    pub tag_ids: Vec<String>,
    /// `owner`, `viewer` or `editor`.
    pub permission: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorNote {
    #[serde(flatten)]
    pub summary: AnchorNoteSummary,
    #[serde(default)]
    pub content: Option<String>,
    pub created_at: String,
    pub is_archived: bool,
    #[serde(default)]
    pub background: Option<String>,
    pub user_id: String,
}

fn bearer(ctx: &IntegrationContext) -> HomarrResult<String> {
    Ok(format!("Bearer {}", ctx.require_secret("apiKey")?))
}

fn permission_is_valid(permission: &str) -> bool {
    matches!(permission, "owner" | "viewer" | "editor")
}

fn validate_summaries(ctx: &IntegrationContext, notes: &[AnchorNoteSummary]) -> HomarrResult<()> {
    match notes
        .iter()
        .find(|note| !permission_is_valid(&note.permission))
    {
        Some(note) => Err(crate::error::parse_error(
            ctx,
            "Anchor notes",
            format!("unknown permission '{}'", note.permission),
        )),
        None => Ok(()),
    }
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let request = rt
        .http
        .standard()
        .get(url(ctx, "/api/notes", &[("limit", super::util::q(1))])?)
        .header("Authorization", bearer(ctx)?);
    let text = send(ctx, request)
        .await?
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    let notes: Vec<AnchorNoteSummary> = parse_json(ctx, "Anchor notes", &text)?;
    validate_summaries(ctx, &notes)
}

#[derive(Debug, Default, Clone)]
pub struct NotesListInput {
    pub search: Option<String>,
    pub tag_id: Option<String>,
    pub limit: Option<u64>,
}

pub async fn list_notes(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    input: &NotesListInput,
) -> HomarrResult<Vec<AnchorNoteSummary>> {
    let trimmed = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let query = [
        ("search", trimmed(&input.search)),
        ("tagId", trimmed(&input.tag_id)),
        ("limit", input.limit.map(|limit| limit.to_string())),
    ];
    let value = request(rt, ctx, Method::GET, url(ctx, "/api/notes", &query)?, None).await?;
    let notes: Vec<AnchorNoteSummary> = super::util::from_value(ctx, "Anchor notes", value)?;
    validate_summaries(ctx, &notes)?;
    Ok(notes)
}

pub async fn get_note(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    note_id: &str,
) -> HomarrResult<AnchorNote> {
    let encoded_id =
        percent_encoding::utf8_percent_encode(note_id, percent_encoding::NON_ALPHANUMERIC);
    let value = request(
        rt,
        ctx,
        Method::GET,
        url(ctx, &format!("/api/notes/{encoded_id}"), &[])?,
        None,
    )
    .await?;
    super::util::from_value(ctx, "Anchor note", value)
}

pub async fn update_note(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    note_id: &str,
    title: Option<&str>,
    content: Option<&str>,
) -> HomarrResult<AnchorNote> {
    if title.is_none() && content.is_none() {
        return Err(HomarrError::BadRequest(
            "At least one field to update must be provided".into(),
        ));
    }
    let mut body = serde_json::Map::new();
    if let Some(title) = title {
        body.insert("title".into(), json!(title));
    }
    if let Some(content) = content {
        body.insert("content".into(), json!(content));
    }
    let encoded_id =
        percent_encoding::utf8_percent_encode(note_id, percent_encoding::NON_ALPHANUMERIC);
    let value = request(
        rt,
        ctx,
        Method::PATCH,
        url(ctx, &format!("/api/notes/{encoded_id}"), &[])?,
        Some(Value::Object(body)),
    )
    .await?;
    if value.is_null() {
        return get_note(rt, ctx, note_id).await;
    }
    super::util::from_value(ctx, "Anchor note", value)
}

/// Returns `Value::Null` for `204 No Content`.
async fn request(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    method: Method,
    url: url::Url,
    body: Option<Value>,
) -> HomarrResult<Value> {
    let mut builder = rt
        .http
        .standard()
        .request(method, url)
        .header("Authorization", bearer(ctx)?);
    if let Some(body) = body {
        builder = builder.json(&body);
    }
    let response = check_status(ctx, send_raw(ctx, builder).await?)?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    let text = response
        .text()
        .await
        .map_err(|err| request_error(ctx, &err))?;
    parse_json(ctx, "Anchor response", &text)
}

pub async fn data(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
    operation: &str,
    input: &Value,
) -> HomarrResult<Value> {
    let note_id = || {
        input_str(input, "noteId")
            .map(str::to_string)
            .ok_or_else(|| HomarrError::BadRequest("noteId is required".into()))
    };
    match operation {
        "listNotes" => {
            let limit = input_u64(input, "limit");
            if limit.is_some_and(|limit| !(1..=200).contains(&limit)) {
                return Err(HomarrError::BadRequest(
                    "limit must be between 1 and 200".into(),
                ));
            }
            let list_input = NotesListInput {
                search: input_str(input, "search").map(str::to_string),
                tag_id: input_str(input, "tagId").map(str::to_string),
                limit,
            };
            to_json(&list_notes(rt, ctx, &list_input).await?)
        }
        "getNote" => to_json(&get_note(rt, ctx, &note_id()?).await?),
        "updateNote" => to_json(
            &update_note(
                rt,
                ctx,
                &note_id()?,
                input_str(input, "title"),
                input_str(input, "content"),
            )
            .await?,
        ),
        other => Err(unknown_operation(ctx, other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_flattens_summary_fields() {
        let note: AnchorNote = serde_json::from_value(json!({
            "id": "1", "title": "T", "updatedAt": "u", "isPinned": true, "tagIds": ["a"],
            "permission": "owner", "content": null, "createdAt": "c", "isArchived": false, "userId": "u1"
        }))
        .unwrap();
        assert_eq!(note.summary.id, "1");
        assert_eq!(note.content, None);
        assert_eq!(serde_json::to_value(&note).unwrap()["tagIds"], json!(["a"]));
    }
}
