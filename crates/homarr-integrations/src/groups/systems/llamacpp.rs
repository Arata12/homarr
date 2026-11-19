//! llama.cpp server (`llama-cpp/llamacpp-integration.ts` + `llamacpp-types.ts`).

use homarr_common::HomarrResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::{self, f64_of};
use crate::error::parse_error;
use crate::runtime::IntegrationRuntime;
use crate::types::IntegrationContext;

const REQUEST_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppModel {
    pub id: String,
    pub name: String,
    pub context_size: Option<f64>,
    pub parameter_count: Option<f64>,
    pub file_size_bytes: Option<f64>,
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppContextUsage {
    pub used_tokens: f64,
    pub context_size: f64,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppSlotsSummary {
    pub total: u32,
    pub processing: u32,
    pub decoding: u32,
    pub speculative: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppPerRequest {
    pub task_id: Option<f64>,
    pub decoded_tokens: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppMetrics {
    pub generation_speed_tps: Option<f64>,
    pub prompt_speed_tps: Option<f64>,
    pub avg_generation_speed_tps: Option<f64>,
    pub avg_prompt_speed_tps: Option<f64>,
    pub requests_processing: Option<f64>,
    pub requests_deferred: Option<f64>,
    pub tokens_processed: Option<f64>,
    pub tokens_generated: Option<f64>,
    pub prompt_cache_hit_rate: Option<f64>,
    pub request_decoded_tokens: Option<f64>,
    pub task_id: Option<f64>,
    pub spec_draft_tokens: Option<f64>,
    pub spec_accepted_tokens: Option<f64>,
    pub spec_drafts: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamacppStats {
    pub health: String,
    pub model: Option<LlamacppModel>,
    pub context_usage: Option<LlamacppContextUsage>,
    pub slots: Option<LlamacppSlotsSummary>,
    pub metrics: LlamacppMetrics,
}

pub async fn test_connection(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<()> {
    let req = http::get(rt, ctx, "/health", &[])?.timeout(http::timeout(REQUEST_TIMEOUT_SECS));
    // The payload shape is validated: an unrelated JSON body must not pass connection testing.
    let value = http::send_json(ctx, req, "llama.cpp /health").await?;
    parse_health(ctx, &value).map(|_| ())
}

pub async fn stats(
    rt: &IntegrationRuntime,
    ctx: &IntegrationContext,
) -> HomarrResult<LlamacppStats> {
    let timeout = http::timeout(REQUEST_TIMEOUT_SECS);
    let (health, models) = tokio::join!(
        http::send_json(
            ctx,
            http::get(rt, ctx, "/health", &[])?.timeout(timeout),
            "llama.cpp /health"
        ),
        http::send_json(
            ctx,
            http::get(rt, ctx, "/v1/models", &[])?.timeout(timeout),
            "llama.cpp /v1/models"
        ),
    );
    let health = parse_health(ctx, &health?)?;
    let models = parse_models(ctx, &models?)?;
    let first_model = models.first();
    let model_query: Vec<(&str, &str)> = first_model
        .map(|m| vec![("model", m.id.as_str())])
        .unwrap_or_default();

    let (metrics_response, slots_response) = tokio::join!(
        http::send_text(
            ctx,
            http::get(rt, ctx, "/metrics", &model_query)?.timeout(timeout)
        ),
        http::send_json(
            ctx,
            http::get(rt, ctx, "/slots", &model_query)?.timeout(timeout),
            "llama.cpp /slots"
        ),
    );
    // /metrics needs `--metrics` (otherwise 501) and /slots is not on every build: both degrade gracefully.
    let metrics = metrics_response
        .map(|text| parse_prometheus_metrics(&text))
        .unwrap_or_default();
    let slots = slots_response.ok().and_then(|v| v.as_array().cloned());

    Ok(map_stats(
        health,
        first_model.cloned(),
        &metrics,
        map_context_usage(slots.as_deref()),
        map_slots_summary(slots.as_deref()),
        map_per_request(slots.as_deref()),
    ))
}

fn parse_health(ctx: &IntegrationContext, value: &Value) -> HomarrResult<String> {
    value
        .get("status")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            parse_error(
                ctx,
                "llama.cpp /health",
                "Invalid llama.cpp /health response",
            )
        })
}

fn short_model_name(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    let lower = last.to_lowercase();
    if lower.ends_with(".gguf") {
        last[..last.len() - 5].to_string()
    } else {
        last.to_string()
    }
}

pub fn parse_models(ctx: &IntegrationContext, value: &Value) -> HomarrResult<Vec<LlamacppModel>> {
    let invalid = || {
        parse_error(
            ctx,
            "llama.cpp /v1/models",
            "Invalid llama.cpp /v1/models response",
        )
    };
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    data.iter()
        .map(|model| {
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            let meta = model.get("meta").filter(|m| m.is_object());
            Ok(LlamacppModel {
                id: id.to_string(),
                name: short_model_name(id),
                context_size: meta.and_then(|m| f64_of(m, "n_ctx")),
                parameter_count: meta.and_then(|m| f64_of(m, "n_params")),
                file_size_bytes: meta.and_then(|m| f64_of(m, "size")),
                quantization: meta
                    .and_then(|m| m.get("ftype"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Parses Prometheus text exposition format into `(name, value)` pairs, skipping comments and non-finite values.
pub fn parse_prometheus_metrics(text: &str) -> Vec<(String, f64)> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let name_end = line
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
                .unwrap_or(line.len());
            let name = &line[..name_end];
            if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                return None;
            }
            let mut rest = &line[name_end..];
            if rest.starts_with('{') {
                rest = &rest[rest.find('}')? + 1..];
            }
            if !rest.starts_with(char::is_whitespace) {
                return None;
            }
            let value: f64 = rest.trim().parse().ok()?;
            value.is_finite().then(|| (name.to_string(), value))
        })
        .collect()
}

fn metric(metrics: &[(String, f64)], name: &str) -> Option<f64> {
    metrics.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
}

/// Cumulative counters averaged (tokens / seconds) so a stable number exists while the server is idle.
fn avg_tps(tokens: Option<f64>, seconds: Option<f64>) -> Option<f64> {
    match (tokens, seconds) {
        (Some(tokens), Some(seconds)) if seconds > 0.0 => Some(tokens / seconds),
        _ => None,
    }
}

/// Context (KV) usage from /slots: the largest used prompt over its slot context size.
pub fn map_context_usage(slots: Option<&[Value]>) -> Option<LlamacppContextUsage> {
    let mut largest: Option<LlamacppContextUsage> = None;
    for slot in slots? {
        let (Some(context_size), Some(used)) = (
            f64_of_num(slot, "n_ctx"),
            f64_of_num(slot, "n_prompt_tokens"),
        ) else {
            continue;
        };
        if context_size > 0.0 && used >= 0.0 {
            let usage = LlamacppContextUsage {
                used_tokens: used,
                context_size,
                percent: ((used / context_size) * 1000.0).round() / 10.0,
            };
            let usage = LlamacppContextUsage {
                percent: usage.percent.clamp(0.0, 100.0),
                ..usage
            };
            if largest
                .as_ref()
                .is_none_or(|l| usage.used_tokens > l.used_tokens)
            {
                largest = Some(usage);
            }
        }
    }
    largest
}

fn f64_of_num(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

pub fn map_slots_summary(slots: Option<&[Value]>) -> Option<LlamacppSlotsSummary> {
    let slots = slots.filter(|s| !s.is_empty())?;
    let (mut total, mut processing, mut decoding) = (0, 0, 0);
    let mut speculative: Option<bool> = None;
    for slot in slots.iter().filter(|s| s.is_object()) {
        total += 1;
        if slot.get("is_processing").and_then(Value::as_bool) == Some(true) {
            processing += 1;
            if slot
                .get("next_token")
                .and_then(Value::as_array)
                .is_some_and(|t| !t.is_empty())
            {
                decoding += 1;
            }
        }
        match slot.get("speculative").and_then(Value::as_bool) {
            Some(true) => speculative = Some(true),
            Some(false) if speculative != Some(true) => speculative = Some(false),
            _ => {}
        }
    }
    (total > 0).then_some(LlamacppSlotsSummary {
        total,
        processing,
        decoding,
        speculative,
    })
}

/// Identity (`id_task`) and generated-token count (`next_token[0].n_decoded`) of the active request.
pub fn map_per_request(slots: Option<&[Value]>) -> LlamacppPerRequest {
    for slot in slots.unwrap_or_default() {
        if slot.get("is_processing").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        return LlamacppPerRequest {
            task_id: f64_of_num(slot, "id_task"),
            decoded_tokens: slot
                .pointer("/next_token/0/n_decoded")
                .and_then(Value::as_f64),
        };
    }
    LlamacppPerRequest::default()
}

pub fn map_stats(
    health: String,
    model: Option<LlamacppModel>,
    metrics: &[(String, f64)],
    context_usage: Option<LlamacppContextUsage>,
    slots: Option<LlamacppSlotsSummary>,
    per_request: LlamacppPerRequest,
) -> LlamacppStats {
    let prompt_tokens = metric(metrics, "llamacpp:prompt_tokens_total");
    let cached_tokens = metric(metrics, "llamacpp:prompt_tokens_cached_total");
    let prompt_cache_hit_rate = match (prompt_tokens, cached_tokens) {
        (Some(prompt), Some(cached)) if prompt + cached > 0.0 => {
            Some((cached / (prompt + cached) * 100.0).round())
        }
        _ => None,
    };
    LlamacppStats {
        health,
        model,
        context_usage,
        slots,
        metrics: LlamacppMetrics {
            generation_speed_tps: metric(metrics, "llamacpp:predicted_tokens_seconds"),
            prompt_speed_tps: metric(metrics, "llamacpp:prompt_tokens_seconds"),
            avg_generation_speed_tps: avg_tps(
                metric(metrics, "llamacpp:tokens_predicted_total"),
                metric(metrics, "llamacpp:tokens_predicted_seconds_total"),
            ),
            avg_prompt_speed_tps: avg_tps(
                metric(metrics, "llamacpp:prompt_tokens_total"),
                metric(metrics, "llamacpp:prompt_seconds_total"),
            ),
            requests_processing: metric(metrics, "llamacpp:requests_processing"),
            requests_deferred: metric(metrics, "llamacpp:requests_deferred"),
            tokens_processed: prompt_tokens,
            tokens_generated: metric(metrics, "llamacpp:tokens_predicted_total"),
            prompt_cache_hit_rate,
            request_decoded_tokens: per_request.decoded_tokens,
            task_id: per_request.task_id,
            spec_draft_tokens: metric(metrics, "llamacpp:spec_decode_num_draft_tokens_total"),
            spec_accepted_tokens: metric(metrics, "llamacpp:spec_decode_num_accepted_tokens_total"),
            spec_drafts: metric(metrics, "llamacpp:spec_decode_num_drafts_total"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use homarr_definitions::IntegrationKind;
    use serde_json::json;

    use super::*;

    fn ctx() -> IntegrationContext {
        IntegrationContext {
            id: "l".into(),
            name: "llama".into(),
            url: "http://llama".into(),
            external_url: None,
            kind: IntegrationKind::Llamacpp,
            secrets: HashMap::new(),
        }
    }

    #[test]
    fn parses_prometheus_text() {
        let text = "# HELP x\n# TYPE x gauge\nllamacpp:requests_processing 2\nllamacpp:predicted_tokens_seconds{model=\"a\"} 12.5\nbad line\nllamacpp:nan NaN\nllamacpp:exp 1e3\n";
        let metrics = parse_prometheus_metrics(text);
        assert_eq!(metric(&metrics, "llamacpp:requests_processing"), Some(2.0));
        assert_eq!(
            metric(&metrics, "llamacpp:predicted_tokens_seconds"),
            Some(12.5)
        );
        assert_eq!(metric(&metrics, "llamacpp:exp"), Some(1000.0));
        assert_eq!(metric(&metrics, "llamacpp:nan"), None);
        assert_eq!(metrics.len(), 3);
    }

    #[test]
    fn maps_models_and_short_names() {
        let models = parse_models(
            &ctx(),
            &json!({"data":[{"id":"org/Qwen-7B.Q4.GGUF","meta":{"n_ctx":4096,"n_params":7e9,"size":4e9,"ftype":"Q4_K_M"}}]}),
        )
        .unwrap();
        assert_eq!(models[0].name, "Qwen-7B.Q4");
        assert_eq!(models[0].context_size, Some(4096.0));
        assert_eq!(models[0].quantization.as_deref(), Some("Q4_K_M"));
        assert!(parse_models(&ctx(), &json!({"nope":1})).is_err());
    }

    #[test]
    fn maps_slots() {
        let slots = vec![
            json!({"id_task": 7, "is_processing": true, "n_ctx": 1000, "n_prompt_tokens": 250, "speculative": false, "next_token":[{"n_decoded": 42}]}),
            json!({"is_processing": false, "n_ctx": 1000, "n_prompt_tokens": 100, "speculative": true}),
        ];
        let usage = map_context_usage(Some(&slots)).unwrap();
        assert_eq!(usage.used_tokens, 250.0);
        assert_eq!(usage.percent, 25.0);
        let summary = map_slots_summary(Some(&slots)).unwrap();
        assert_eq!(
            (summary.total, summary.processing, summary.decoding),
            (2, 1, 1)
        );
        assert_eq!(summary.speculative, Some(true));
        let per_request = map_per_request(Some(&slots));
        assert_eq!(per_request.task_id, Some(7.0));
        assert_eq!(per_request.decoded_tokens, Some(42.0));
        assert!(map_slots_summary(Some(&[])).is_none());
        assert!(map_context_usage(None).is_none());
    }

    #[test]
    fn maps_stats_with_derived_values() {
        let metrics = vec![
            ("llamacpp:prompt_tokens_total".to_string(), 300.0),
            ("llamacpp:prompt_tokens_cached_total".to_string(), 100.0),
            ("llamacpp:tokens_predicted_total".to_string(), 90.0),
            ("llamacpp:tokens_predicted_seconds_total".to_string(), 9.0),
        ];
        let stats = map_stats(
            "ok".into(),
            None,
            &metrics,
            None,
            None,
            LlamacppPerRequest::default(),
        );
        assert_eq!(stats.metrics.prompt_cache_hit_rate, Some(25.0));
        assert_eq!(stats.metrics.avg_generation_speed_tps, Some(10.0));
        assert_eq!(stats.metrics.avg_prompt_speed_tps, None);
        assert_eq!(stats.metrics.tokens_processed, Some(300.0));
    }
}
