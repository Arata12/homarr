use homarr_common::{HomarrError, HomarrResult, SecretCrypto};
use homarr_core::{AppConfig, HttpClientFactory};
use homarr_db::{
    models::{AssistantConfigRecord, AssistantMessageRecord},
    repos::assistant as repo,
    Database,
};
use serde_json::{json, Value};

#[derive(Clone)]
pub struct AssistantService {
    config: AppConfig,
    crypto: SecretCrypto,
    http: HttpClientFactory,
}

impl AssistantService {
    pub fn new(config: AppConfig, crypto: SecretCrypto, http: HttpClientFactory) -> Self {
        Self {
            config,
            crypto,
            http,
        }
    }

    pub async fn send_user_message(
        &self,
        db: &Database,
        thread_id: &str,
        prompt: &str,
    ) -> HomarrResult<(AssistantMessageRecord, AssistantMessageRecord)> {
        let user_payload = json!({ "role": "user", "text": prompt }).to_string();
        let user_msg = repo::add_message(db, thread_id, None, &user_payload).await?;

        let cfg = repo::get_config(db).await?;
        let history = repo::list_messages(db, thread_id).await?;
        let reply_text = self.generate_reply(&cfg, &history, prompt).await?;

        let assistant_payload = json!({ "role": "assistant", "text": reply_text }).to_string();
        let assistant_msg =
            repo::add_message(db, thread_id, Some(&user_msg.id), &assistant_payload).await?;
        Ok((user_msg, assistant_msg))
    }

    async fn generate_reply(
        &self,
        cfg: &AssistantConfigRecord,
        history: &[AssistantMessageRecord],
        prompt: &str,
    ) -> HomarrResult<String> {
        let api_key = cfg
            .encrypted_api_key
            .as_deref()
            .and_then(|enc| self.crypto.decrypt_secret(enc).ok())
            .filter(|k| !k.trim().is_empty());

        if (!cfg.enabled || api_key.is_none() || self.config.no_external_connection)
            && self.config.demo_mode
        {
            return Ok(format!(
                "Homarr Demo Assistant: Received \"{}\". Configure an LLM provider key under /manage/assistant for live model completions.",
                prompt.trim()
            ));
        }

        if !cfg.enabled {
            return Err(HomarrError::Forbidden(
                "Assistant is disabled in server configuration".into(),
            ));
        }

        let base_url = cfg.base_url.trim_end_matches('/');
        let model = cfg
            .model_id
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o-mini".to_string());

        let messages: Vec<Value> = history
            .iter()
            .filter_map(|m| {
                let parsed: Value = serde_json::from_str(&m.content).ok()?;
                let role = parsed.get("role")?.as_str()?;
                let text = parsed.get("text")?.as_str()?;
                Some(json!({ "role": role, "content": text }))
            })
            .collect();

        if cfg.provider == "anthropic" {
            let key = api_key
                .ok_or_else(|| HomarrError::BadRequest("Anthropic API key is required".into()))?;
            let resp = self
                .http
                .standard()
                .post(format!("{base_url}/messages"))
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .json(&json!({
                    "model": model,
                    "max_tokens": 1024,
                    "messages": messages,
                }))
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("Anthropic request failed: {e}")))?;
            let body: Value = resp
                .json()
                .await
                .map_err(|e| HomarrError::Upstream(e.to_string()))?;
            let text = body
                .get("content")
                .and_then(|c| c.as_array())
                .and_then(|a| a.first())
                .and_then(|x| x.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("No response returned by provider.")
                .to_string();
            return Ok(text);
        }

        if cfg.provider == "ollama" {
            let resp = self
                .http
                .standard()
                .post(format!("{base_url}/api/chat"))
                .json(&json!({
                    "model": model,
                    "stream": false,
                    "messages": messages,
                }))
                .send()
                .await
                .map_err(|e| HomarrError::Upstream(format!("Ollama request failed: {e}")))?;
            let body: Value = resp
                .json()
                .await
                .map_err(|e| HomarrError::Upstream(e.to_string()))?;
            let text = body
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("No response returned by Ollama.")
                .to_string();
            return Ok(text);
        }

        let mut req = self
            .http
            .standard()
            .post(format!("{base_url}/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": messages,
            }));
        if let Some(key) = api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| HomarrError::Upstream(format!("LLM request failed: {e}")))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| HomarrError::Upstream(e.to_string()))?;
        if !status.is_success() {
            return Err(HomarrError::Upstream(format!(
                "LLM provider returned HTTP {status}: {body}"
            )));
        }
        let text = body
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("No completion returned by provider.")
            .to_string();
        Ok(text)
    }
}
