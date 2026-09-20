//! Sole egress proxy for Helix.
//!
//! Hands and Loom must not raw-dial the network. All outbound HTTP goes through
//! a `Switch` whose allowlist is derived from the active charter pack.
//!
//! - **hearthside**: Ollama loopback only (`127.0.0.1` / `localhost` / `::1`).
//! - **desk**: same plus future read-only connector hosts (none yet).
//! - **workshop**: desk plus optional cloud Loom hosts (none hardcoded yet;
//!   still blocked until an explicit allow entry is added).

use std::collections::BTreeSet;

use helix_charter::Charter;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum SwitchError {
    #[error("egress denied: {0}")]
    Denied(String),
    #[error("invalid url: {0}")]
    InvalidUrl(String),
}

/// Destination class for policy decisions and status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestClass {
    /// Local Ollama (or other loopback model runtime).
    LocalModel,
    /// Explicitly allowlisted non-loopback host (future cloud / connectors).
    AllowedRemote,
    /// Everything else.
    Blocked,
}

/// Charter-derived allowlist. Stateless; rebuilt when the pack changes.
#[derive(Debug, Clone)]
pub struct Switch {
    pack: String,
    /// Hosts (lowercase) that may be contacted when the destination is not
    /// already classified as loopback local-model.
    allowed_hosts: BTreeSet<String>,
    allow_local_model: bool,
    allow_cloud_model: bool,
    allow_network_adapters: bool,
}

impl Switch {
    /// Build the gate from a loaded charter.
    pub fn from_charter(charter: &Charter) -> Self {
        let mut allowed_hosts = BTreeSet::new();
        // No remote hosts are pre-seeded in this slice. Cloud Loom and
        // connector endpoints will be added explicitly when those adapters ship.
        // Workshop's `allow_cloud_model` does not open the internet by itself.
        let _ = charter.allow_cloud_model;
        let _ = charter.allow_network_adapters;

        Self {
            pack: charter.pack.clone(),
            allowed_hosts,
            allow_local_model: charter.allow_local_model,
            allow_cloud_model: charter.allow_cloud_model,
            allow_network_adapters: charter.allow_network_adapters,
        }
    }

    /// Convenience: builtin pack name → Switch.
    pub fn for_pack(pack: &str) -> Result<Self, helix_charter::CharterError> {
        let c = Charter::builtin(pack)?;
        Ok(Self::from_charter(&c))
    }

    pub fn pack(&self) -> &str {
        &self.pack
    }

    /// Classify a URL without performing I/O.
    pub fn classify(&self, url_str: &str) -> Result<DestClass, SwitchError> {
        let url = Url::parse(url_str).map_err(|e| SwitchError::InvalidUrl(e.to_string()))?;
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(SwitchError::Denied(format!(
                "scheme {scheme} not allowed (only http/https)"
            )));
        }
        let host = url
            .host_str()
            .ok_or_else(|| SwitchError::Denied("url has no host".into()))?
            .to_ascii_lowercase();

        if is_loopback_host(&host) {
            // Loopback is only for the local model runtime in current packs.
            if self.allow_local_model {
                return Ok(DestClass::LocalModel);
            }
            return Err(SwitchError::Denied(
                "local model not allowed by charter".into(),
            ));
        }

        if self.allowed_hosts.contains(&host) {
            return Ok(DestClass::AllowedRemote);
        }

        Ok(DestClass::Blocked)
    }

    /// Return Ok(()) if the URL may be dialed; otherwise a denial reason.
    pub fn check(&self, url_str: &str) -> Result<(), SwitchError> {
        match self.classify(url_str)? {
            DestClass::LocalModel | DestClass::AllowedRemote => Ok(()),
            DestClass::Blocked => Err(SwitchError::Denied(format!(
                "host not on allowlist for pack {} (url={url_str})", self.pack
            ))),
        }
    }

    /// Human-readable summary for status / CLI.
    pub fn summary(&self) -> String {
        let mut lines = vec![format!("pack={}", self.pack)];
        if self.allow_local_model {
            lines.push("local_model=loopback-only".into());
        } else {
            lines.push("local_model=denied".into());
        }
        if self.allow_cloud_model {
            lines.push("cloud_model=flag-on (no hosts seeded yet)".into());
        } else {
            lines.push("cloud_model=denied".into());
        }
        if self.allow_network_adapters {
            lines.push("network_adapters=flag-on (no hosts seeded yet)".into());
        } else {
            lines.push("network_adapters=denied".into());
        }
        if self.allowed_hosts.is_empty() {
            lines.push("allowed_hosts=(none)".into());
        } else {
            lines.push(format!(
                "allowed_hosts={}",
                self.allowed_hosts.iter().cloned().collect::<Vec<_>>().join(",")
            ));
        }
        lines.join(" ")
    }
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
        || host.starts_with("127.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hearthside_allows_ollama_loopback() {
        let s = Switch::for_pack("hearthside").unwrap();
        s.check("http://127.0.0.1:11434/api/generate").unwrap();
        s.check("http://localhost:11434/api/tags").unwrap();
        assert_eq!(
            s.classify("http://127.0.0.1:11434").unwrap(),
            DestClass::LocalModel
        );
    }

    #[test]
    fn hearthside_blocks_remote() {
        let s = Switch::for_pack("hearthside").unwrap();
        assert!(matches!(
            s.check("https://api.openai.com/v1/chat/completions"),
            Err(SwitchError::Denied(_))
        ));
        assert!(matches!(
            s.check("http://example.com/"),
            Err(SwitchError::Denied(_))
        ));
        assert_eq!(
            s.classify("https://api.openai.com").unwrap(),
            DestClass::Blocked
        );
    }

    #[test]
    fn workshop_still_blocks_unlisted_cloud() {
        // Flag is on, but no hosts are seeded — Switch stays closed.
        let s = Switch::for_pack("workshop").unwrap();
        assert!(matches!(
            s.check("https://api.openai.com/v1/chat/completions"),
            Err(SwitchError::Denied(_))
        ));
        s.check("http://127.0.0.1:11434/api/tags").unwrap();
    }

    #[test]
    fn non_http_scheme_denied() {
        let s = Switch::for_pack("hearthside").unwrap();
        assert!(matches!(
            s.check("ftp://127.0.0.1/"),
            Err(SwitchError::Denied(_))
        ));
    }
}
