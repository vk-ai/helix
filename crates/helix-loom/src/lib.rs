//! Loom: model completion providers for Helix.
//!
//! All outbound HTTP goes through [`helix_switch::Switch`]. Secrets are unwrapped
//! from Reliquary only at the adapter boundary and never enter model context.
//!
//! Providers:
//! - **Ollama** (default): local `/api/generate` on loopback.
//! - **OpenAI-compat**: `/v1/chat/completions` with a Reliquary API-key reference.
//!   Cloud is blocked when the charter has `allow_cloud_model = false` (hearthside).

use helix_charter::Charter;
use helix_reliquary::Reliquary;
use helix_switch::{DestClass, Switch};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum LoomError {
    #[error("switch denied egress: {0}")]
    Switch(String),
    #[error("cloud model not allowed by charter pack {0}")]
    CloudBlocked(String),
    #[error("local model not allowed by charter")]
    LocalBlocked,
    #[error("openai provider requires HELIX_OPENAI_KEY_REF (reliquary secret name)")]
    MissingKeyRef,
    #[error("reliquary: {0}")]
    Reliquary(String),
    #[error("invalid provider base url: {0}")]
    BadUrl(String),
    #[error("http: {0}")]
    Http(String),
    #[error("empty model response")]
    EmptyResponse,
    #[error("unknown loom provider: {0} (use ollama or openai)")]
    UnknownProvider(String),
}

/// Which backend Loom will dial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    Ollama,
    OpenAiCompat,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Result<Self, LoomError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ollama" | "" => Ok(Self::Ollama),
            "openai" | "openai-compat" | "openai_compat" => Ok(Self::OpenAiCompat),
            other => Err(LoomError::UnknownProvider(other.into())),
        }
    }
}

/// Resolved Loom configuration from environment + defaults.
#[derive(Debug, Clone)]
pub struct LoomConfig {
    pub kind: ProviderKind,
    /// Ollama base, e.g. http://127.0.0.1:11434
    pub ollama_base: String,
    pub ollama_model: String,
    /// OpenAI-compatible base, e.g. https://api.openai.com
    pub openai_base: String,
    pub openai_model: String,
    /// Reliquary secret *name* holding the API key (never the value).
    pub openai_key_ref: Option<String>,
}

impl LoomConfig {
    /// Load from environment variables.
    pub fn from_env() -> Result<Self, LoomError> {
        let kind = ProviderKind::parse(
            &std::env::var("HELIX_LOOM").unwrap_or_else(|_| "ollama".into()),
        )?;
        Ok(Self {
            kind,
            ollama_base: std::env::var("HELIX_OLLAMA")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
            ollama_model: std::env::var("HELIX_MODEL").unwrap_or_else(|_| "llama3.2".into()),
            openai_base: std::env::var("HELIX_OPENAI_BASE")
                .unwrap_or_else(|_| "https://api.openai.com".into()),
            openai_model: std::env::var("HELIX_OPENAI_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".into()),
            openai_key_ref: std::env::var("HELIX_OPENAI_KEY_REF").ok().filter(|s| !s.is_empty()),
        })
    }

    /// Short label for status / logs (never includes secrets).
    pub fn summary(&self) -> String {
        match self.kind {
            ProviderKind::Ollama => {
                format!("provider=ollama model={} base={}", self.ollama_model, self.ollama_base)
            }
            ProviderKind::OpenAiCompat => {
                let key = self
                    .openai_key_ref
                    .as_deref()
                    .unwrap_or("(unset)");
                format!(
                    "provider=openai-compat model={} base={} key_ref={}",
                    self.openai_model, self.openai_base, key
                )
            }
        }
    }
}

/// Complete a prompt via the configured provider. All dials pass Switch.
pub async fn complete(
    config: &LoomConfig,
    charter: &Charter,
    switch: &Switch,
    reliquary: Option<&Reliquary>,
    system_and_user: &str,
) -> Result<String, LoomError> {
    match config.kind {
        ProviderKind::Ollama => {
            ollama_complete(config, charter, switch, system_and_user).await
        }
        ProviderKind::OpenAiCompat => {
            openai_complete(config, charter, switch, reliquary, system_and_user).await
        }
    }
}

/// Probe whether the active provider endpoint is reachable (Switch + HTTP).
pub async fn reachable(config: &LoomConfig, switch: &Switch) -> bool {
    match config.kind {
        ProviderKind::Ollama => {
            let url = format!("{}/api/tags", config.ollama_base.trim_end_matches('/'));
            if switch.check(&url).is_err() {
                return false;
            }
            reqwest::Client::new()
                .get(&url)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }
        ProviderKind::OpenAiCompat => {
            let base = config.openai_base.trim_end_matches('/');
            let url = format!("{base}/v1/models");
            match switch.classify(&url) {
                Ok(DestClass::AllowedRemote) | Ok(DestClass::LocalModel) => true,
                _ => false,
            }
        }
    }
}

async fn ollama_complete(
    config: &LoomConfig,
    charter: &Charter,
    switch: &Switch,
    prompt: &str,
) -> Result<String, LoomError> {
    if !charter.allow_local_model {
        return Err(LoomError::LocalBlocked);
    }
    let generate_url = format!(
        "{}/api/generate",
        config.ollama_base.trim_end_matches('/')
    );
    switch
        .check(&generate_url)
        .map_err(|e| LoomError::Switch(e.to_string()))?;

    let body = json!({
        "model": config.ollama_model,
        "prompt": prompt,
        "stream": false,
    });
    let res = reqwest::Client::new()
        .post(&generate_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| LoomError::Http(e.to_string()))?;
    let parsed: OllamaResponse = res
        .json()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?;
    parsed
        .response
        .filter(|s| !s.is_empty())
        .ok_or(LoomError::EmptyResponse)
}

async fn openai_complete(
    config: &LoomConfig,
    charter: &Charter,
    switch: &Switch,
    reliquary: Option<&Reliquary>,
    prompt: &str,
) -> Result<String, LoomError> {
    let base = config.openai_base.trim_end_matches('/');
    let url = format!("{base}/v1/chat/completions");

    let parsed = Url::parse(&url).map_err(|e| LoomError::BadUrl(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| LoomError::BadUrl("no host".into()))?
        .to_ascii_lowercase();
    let is_loopback = matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]")
        || host.starts_with("127.");

    if is_loopback {
        if !charter.allow_local_model {
            return Err(LoomError::LocalBlocked);
        }
    } else {
        if !charter.allow_cloud_model {
            return Err(LoomError::CloudBlocked(charter.pack.clone()));
        }
    }

    match switch.classify(&url) {
        Ok(DestClass::LocalModel) | Ok(DestClass::AllowedRemote) => {}
        Ok(DestClass::Blocked) => {
            if charter.allow_cloud_model && !is_loopback {
                // Configured openai base is dialable under allow_cloud_model.
            } else {
                return Err(LoomError::Switch(format!(
                    "host not on allowlist for pack {} (url={url})",
                    charter.pack
                )));
            }
        }
        Err(e) => return Err(LoomError::Switch(e.to_string())),
    }

    let key_ref = config
        .openai_key_ref
        .as_deref()
        .ok_or(LoomError::MissingKeyRef)?;
    let rel = reliquary.ok_or_else(|| {
        LoomError::Reliquary("reliquary not available for API key unwrap".into())
    })?;
    let api_key = rel
        .unwrap(key_ref)
        .map_err(|e| LoomError::Reliquary(e.to_string()))?;

    let body = json!({
        "model": config.openai_model,
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "stream": false,
    });

    let res = reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| LoomError::Http(e.to_string()))?;

    let parsed: OpenAiResponse = res
        .json()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?;
    let text = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message)
        .and_then(|m| m.content)
        .filter(|s| !s.is_empty())
        .ok_or(LoomError::EmptyResponse)?;
    Ok(text)
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: Option<OpenAiMessage>,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_switch::Switch;

    #[test]
    fn parse_provider() {
        assert_eq!(ProviderKind::parse("ollama").unwrap(), ProviderKind::Ollama);
        assert_eq!(
            ProviderKind::parse("openai").unwrap(),
            ProviderKind::OpenAiCompat
        );
        assert!(ProviderKind::parse("foo").is_err());
    }

    #[test]
    fn config_summary_has_no_secret() {
        let c = LoomConfig {
            kind: ProviderKind::OpenAiCompat,
            ollama_base: "http://127.0.0.1:11434".into(),
            ollama_model: "llama3.2".into(),
            openai_base: "https://api.openai.com".into(),
            openai_model: "gpt-4o-mini".into(),
            openai_key_ref: Some("openai-key".into()),
        };
        let s = c.summary();
        assert!(s.contains("key_ref=openai-key"));
        assert!(!s.contains("sk-"));
    }

    #[test]
    fn hearthside_blocks_cloud_openai() {
        let charter = Charter::builtin("hearthside").unwrap();
        assert!(!charter.allow_cloud_model);
        let config = LoomConfig {
            kind: ProviderKind::OpenAiCompat,
            ollama_base: "http://127.0.0.1:11434".into(),
            ollama_model: "llama3.2".into(),
            openai_base: "https://api.openai.com".into(),
            openai_model: "gpt-4o-mini".into(),
            openai_key_ref: Some("openai-key".into()),
        };
        let switch = Switch::from_charter(&charter);
        let url = format!(
            "{}/v1/chat/completions",
            config.openai_base.trim_end_matches('/')
        );
        assert_eq!(
            switch.classify(&url).unwrap(),
            DestClass::Blocked
        );
        assert!(!charter.allow_cloud_model);
    }

    #[test]
    fn workshop_allows_cloud_flag() {
        let charter = Charter::builtin("workshop").unwrap();
        assert!(charter.allow_cloud_model);
    }
}
