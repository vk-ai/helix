//! Loom: model provider interface for Helix.
//!
//! All outbound HTTP goes through [`Switch`]. Secret values are unwrapped from
//! Reliquary only at the provider boundary and never returned to callers in a
//! form that would enter model context as a credential.
//!
//! Providers:
//! - **Ollama** — local loopback runtime (default on every pack that allows local model).
//! - **OpenAI-compat** — optional chat-completions endpoint; requires charter
//!   `allow_cloud_model`, Switch allowlist entry, and a Reliquary API-key reference.
//!   Blocked on hearthside (and any pack without cloud + seeded host).

use helix_charter::Charter;
use helix_reliquary::Reliquary;
use helix_switch::{Switch, SwitchError};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

/// How Loom was selected for this completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Ollama,
    OpenAiCompat,
}

#[derive(Debug, Error)]
pub enum LoomError {
    #[error("local model not allowed by charter")]
    LocalModelDenied,
    #[error("cloud model not allowed by charter")]
    CloudModelDenied,
    #[error("switch denied egress: {0}")]
    Switch(#[from] SwitchError),
    #[error("missing Reliquary API key reference (set HELIX_OPENAI_KEY_REF and helix secrets add …)")]
    MissingKeyRef,
    #[error("reliquary: {0}")]
    Reliquary(String),
    #[error("http: {0}")]
    Http(String),
    #[error("empty model response")]
    EmptyResponse,
    #[error("no loom provider configured")]
    NoProvider,
}

/// Configuration for Loom (env + defaults). Values only; no secrets.
#[derive(Debug, Clone)]
pub struct LoomConfig {
    pub model: String,
    pub ollama_base: String,
    /// Optional OpenAI-compatible base URL (e.g. `https://api.openai.com/v1`).
    pub openai_base: Option<String>,
    /// Reliquary secret *name* for the API key (never the value).
    pub openai_key_ref: Option<String>,
    /// Force provider: `ollama` | `openai` | auto.
    pub prefer: PreferProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferProvider {
    Auto,
    Ollama,
    OpenAi,
}

impl LoomConfig {
    /// Build from environment variables used by helixd.
    pub fn from_env(model: &str, ollama_base: &str) -> Self {
        let openai_base = std::env::var("HELIX_OPENAI_BASE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let openai_key_ref = std::env::var("HELIX_OPENAI_KEY_REF")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let prefer = match std::env::var("HELIX_LOOM")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "openai" | "openai-compat" | "cloud" => PreferProvider::OpenAi,
            "ollama" | "local" => PreferProvider::Ollama,
            _ => PreferProvider::Auto,
        };
        Self {
            model: model.to_string(),
            ollama_base: ollama_base.trim_end_matches('/').to_string(),
            openai_base,
            openai_key_ref,
            prefer,
        }
    }
}

/// Completion request shared by providers.
#[derive(Debug, Clone)]
pub struct CompleteRequest<'a> {
    pub system_and_user_prompt: &'a str,
    pub model: &'a str,
}

/// Result of a Loom completion.
#[derive(Debug, Clone)]
pub struct CompleteResponse {
    pub text: String,
    pub provider: ProviderKind,
}

/// Run a completion through the selected provider.
///
/// Selection rules:
/// 1. If `prefer` is Ollama (or Auto without cloud config) → Ollama, when
///    `charter.allow_local_model`.
/// 2. If `prefer` is OpenAi, or Auto with `openai_base` + key ref and charter
///    allows cloud → OpenAI-compat (Switch must allow the host).
/// 3. Hearthside never opens cloud: `allow_cloud_model` is false.
pub async fn complete(
    charter: &Charter,
    switch: &Switch,
    config: &LoomConfig,
    reliquary: Option<&Reliquary>,
    prompt: &str,
) -> Result<CompleteResponse, LoomError> {
    let use_openai = match config.prefer {
        PreferProvider::OpenAi => true,
        PreferProvider::Ollama => false,
        PreferProvider::Auto => {
            config.openai_base.is_some()
                && config.openai_key_ref.is_some()
                && charter.allow_cloud_model
        }
    };

    if use_openai {
        complete_openai(charter, switch, config, reliquary, prompt).await
    } else {
        complete_ollama(charter, switch, config, prompt).await
    }
}

async fn complete_ollama(
    charter: &Charter,
    switch: &Switch,
    config: &LoomConfig,
    prompt: &str,
) -> Result<CompleteResponse, LoomError> {
    if !charter.allow_local_model {
        return Err(LoomError::LocalModelDenied);
    }
    let generate_url = format!("{}/api/generate", config.ollama_base);
    switch.check(&generate_url)?;

    let body = json!({
        "model": config.model,
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

    #[derive(Deserialize)]
    struct OllamaResponse {
        response: Option<String>,
    }
    let parsed: OllamaResponse = res
        .json()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?;
    let text = parsed
        .response
        .filter(|s| !s.is_empty())
        .ok_or(LoomError::EmptyResponse)?;
    Ok(CompleteResponse {
        text,
        provider: ProviderKind::Ollama,
    })
}

async fn complete_openai(
    charter: &Charter,
    switch: &Switch,
    config: &LoomConfig,
    reliquary: Option<&Reliquary>,
    prompt: &str,
) -> Result<CompleteResponse, LoomError> {
    if !charter.allow_cloud_model {
        return Err(LoomError::CloudModelDenied);
    }
    let base = config
        .openai_base
        .as_deref()
        .ok_or(LoomError::NoProvider)?
        .trim_end_matches('/');
    let key_ref = config
        .openai_key_ref
        .as_deref()
        .ok_or(LoomError::MissingKeyRef)?;

    let url = format!("{base}/chat/completions");
    switch.check(&url)?;

    let api_key = match reliquary {
        Some(r) => r
            .unwrap(key_ref)
            .map_err(|e| LoomError::Reliquary(e.to_string()))?,
        None => {
            return Err(LoomError::Reliquary(
                "reliquary not available for API key unwrap".into(),
            ))
        }
    };
    // api_key lives only in this stack frame; not logged, not returned.

    let body = json!({
        "model": config.model,
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

    #[derive(Deserialize)]
    struct ChatMessage {
        content: Option<String>,
    }
    #[derive(Deserialize)]
    struct ChatChoice {
        message: Option<ChatMessage>,
    }
    #[derive(Deserialize)]
    struct ChatResponse {
        choices: Option<Vec<ChatChoice>>,
    }

    let parsed: ChatResponse = res
        .json()
        .await
        .map_err(|e| LoomError::Http(e.to_string()))?;
    let text = parsed
        .choices
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.message)
        .and_then(|m| m.content)
        .filter(|s| !s.is_empty())
        .ok_or(LoomError::EmptyResponse)?;

    Ok(CompleteResponse {
        text,
        provider: ProviderKind::OpenAiCompat,
    })
}

/// Probe whether the local Ollama base is reachable (after Switch check).
pub async fn ollama_reachable(switch: &Switch, ollama_base: &str) -> bool {
    let url = format!("{}/api/tags", ollama_base.trim_end_matches('/'));
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

/// Build the constrained Helix system prompt used by ask.
pub fn build_ask_prompt(charter: &Charter, memory: &str, user: &str) -> String {
    format!(
        "You are Helix, a local personal agent.\n\
         You have no tools and no secrets in this slice.\n\
         Charter pack: {}\n{}\n\n\
         Retrieved memory:\n{}\n\n\
         User:\n{}\n\n\
         Reply helpfully. Do not invent capabilities you do not have.\n",
        charter.pack, charter.summary, memory, user
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_switch::Switch;

    #[test]
    fn hearthside_rejects_openai_path() {
        let charter = Charter::builtin("hearthside").unwrap();
        let switch = Switch::from_charter(&charter);
        let config = LoomConfig {
            model: "gpt-4o-mini".into(),
            ollama_base: "http://127.0.0.1:11434".into(),
            openai_base: Some("https://api.openai.com/v1".into()),
            openai_key_ref: Some("openai-key".into()),
            prefer: PreferProvider::OpenAi,
        };
        // Sync check of policy only — we don't run the async body in this unit test.
        assert!(!charter.allow_cloud_model);
        assert!(matches!(
            switch.check("https://api.openai.com/v1/chat/completions"),
            Err(_)
        ));
        let _ = (charter, switch, config);
    }

    #[test]
    fn ollama_url_passes_hearthside_switch() {
        let switch = Switch::for_pack("hearthside").unwrap();
        assert!(switch
            .check("http://127.0.0.1:11434/api/generate")
            .is_ok());
    }

    #[test]
    fn config_from_env_defaults() {
        let c = LoomConfig::from_env("llama3.2", "http://127.0.0.1:11434/");
        assert_eq!(c.ollama_base, "http://127.0.0.1:11434");
        assert_eq!(c.prefer, PreferProvider::Auto);
    }

    #[test]
    fn build_prompt_includes_charter() {
        let c = Charter::builtin("hearthside").unwrap();
        let p = build_ask_prompt(&c, "(none)", "hello");
        assert!(p.contains("hearthside"));
        assert!(p.contains("hello"));
    }
}
