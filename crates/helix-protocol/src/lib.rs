use serde::{Deserialize, Serialize};

pub const DEFAULT_BIND: &str = "127.0.0.1:7420";
pub const DEFAULT_MODEL: &str = "llama3.2";
pub const DEFAULT_OLLAMA: &str = "http://127.0.0.1:11434";
pub const DEFAULT_PACK: &str = "hearthside";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub home: String,
    pub bind: String,
    pub pack: String,
    pub model: String,
    pub ollama: String,
    pub ollama_reachable: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskRequest {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskResponse {
    pub pack: String,
    pub memory_context: String,
    pub reply: String,
    pub model_used: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}
