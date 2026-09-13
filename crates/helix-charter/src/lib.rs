use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CharterError {
    #[error("unknown charter pack: {0}")]
    UnknownPack(String),
    #[error("invalid charter: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Charter {
    pub pack: String,
    pub summary: String,
    pub allow_local_model: bool,
    pub allow_cloud_model: bool,
    pub allow_network_adapters: bool,
    pub allow_shell: bool,
    pub writes_require_ask: bool,
}

impl Charter {
    pub fn builtin(pack: &str) -> Result<Self, CharterError> {
        match pack {
            "hearthside" => Ok(Self {
                pack: pack.into(),
                summary: "Chat, local model, and the plot workspace. No network adapters, no shell, no cloud Loom.".into(),
                allow_local_model: true,
                allow_cloud_model: false,
                allow_network_adapters: false,
                allow_shell: false,
                writes_require_ask: true,
            }),
            "desk" => Ok(Self {
                pack: pack.into(),
                summary: "Hearthside plus future read-only connectors. Writes require an Ask grant in the app.".into(),
                allow_local_model: true,
                allow_cloud_model: false,
                allow_network_adapters: true,
                allow_shell: false,
                writes_require_ask: true,
            }),
            "workshop" => Ok(Self {
                pack: pack.into(),
                summary: "Desk plus plot-scoped shell and optional cloud Loom. Highest power; review Chronicle often.".into(),
                allow_local_model: true,
                allow_cloud_model: true,
                allow_network_adapters: true,
                allow_shell: true,
                writes_require_ask: true,
            }),
            other => Err(CharterError::UnknownPack(other.into())),
        }
    }

    pub fn from_toml(text: &str) -> Result<Self, CharterError> {
        let file: CharterFile = toml::from_str(text)?;
        let mut c = Self::builtin(&file.pack)?;
        if let Some(s) = file.summary {
            c.summary = s;
        }
        Ok(c)
    }

    pub fn to_toml(&self) -> String {
        format!(
            "pack = \"{}\"\n# {}\n",
            self.pack,
            self.summary.replace('\n', " ")
        )
    }
}

#[derive(Debug, Deserialize)]
struct CharterFile {
    pack: String,
    summary: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearthside_has_no_shell() {
        let c = Charter::builtin("hearthside").unwrap();
        assert!(!c.allow_shell);
        assert!(!c.allow_cloud_model);
    }
}
