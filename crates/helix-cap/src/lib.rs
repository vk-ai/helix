//! Shrink-only capability tokens for Helix.
//!
//! Tokens are issued by helixd, signed with an ephemeral session MAC key,
//! and may only be attenuated (rights removed, never added).

use std::collections::BTreeSet;

use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum CapError {
    #[error("unknown right: {0}")]
    UnknownRight(String),
    #[error("cannot widen rights")]
    Widen,
    #[error("invalid token: {0}")]
    Invalid(String),
    #[error("token expired")]
    Expired,
    #[error("token MAC mismatch")]
    BadMac,
}

/// Canonical right strings (aligned with charter flags).
pub mod rights {
    pub const LOCAL_MODEL: &str = "local.model";
    pub const PLOT_READ: &str = "plot.read";
    pub const PLOT_WRITE: &str = "plot.write";
    pub const NETWORK: &str = "network.adapter";
    pub const SHELL: &str = "shell";
    pub const CLOUD_MODEL: &str = "cloud.model";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapToken {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub issued_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub rights: BTreeSet<String>,
    pub mac: String,
}

/// Issues and verifies tokens for one daemon session.
#[derive(Clone)]
pub struct CapAuthority {
    key: Vec<u8>,
}

impl CapAuthority {
    pub fn new(key: Vec<u8>) -> Self {
        Self { key }
    }

    pub fn random() -> Self {
        use rand::RngCore;
        let mut key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Self { key }
    }

    /// Issue a token with the given rights (must be known).
    pub fn issue(
        &self,
        rights: BTreeSet<String>,
        ttl_secs: Option<u64>,
    ) -> Result<CapToken, CapError> {
        for r in &rights {
            validate_right(r)?;
        }
        let id = format!("t-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let issued_at = Utc::now().to_rfc3339();
        let expires_at = ttl_secs.map(|s| (Utc::now() + Duration::seconds(s as i64)).to_rfc3339());
        let mut token = CapToken {
            id,
            parent: None,
            issued_at,
            expires_at,
            rights,
            mac: String::new(),
        };
        token.mac = self.mac_hex(&token)?;
        Ok(token)
    }

    /// Attenuate: keep only a subset of parent rights (no widen).
    pub fn attenuate(
        &self,
        parent: &CapToken,
        keep: BTreeSet<String>,
        ttl_secs: Option<u64>,
    ) -> Result<CapToken, CapError> {
        self.verify(parent)?;
        if !keep.is_subset(&parent.rights) {
            return Err(CapError::Widen);
        }
        for r in &keep {
            validate_right(r)?;
        }
        let id = format!("t-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let issued_at = Utc::now().to_rfc3339();
        let expires_at = ttl_secs.map(|s| (Utc::now() + Duration::seconds(s as i64)).to_rfc3339());
        let mut token = CapToken {
            id,
            parent: Some(parent.id.clone()),
            issued_at,
            expires_at,
            rights: keep,
            mac: String::new(),
        };
        token.mac = self.mac_hex(&token)?;
        Ok(token)
    }

    pub fn verify(&self, token: &CapToken) -> Result<(), CapError> {
        if let Some(exp) = &token.expires_at {
            let exp_t = chrono::DateTime::parse_from_rfc3339(exp)
                .map_err(|e| CapError::Invalid(e.to_string()))?;
            if Utc::now() > exp_t {
                return Err(CapError::Expired);
            }
        }
        let expected = self.mac_hex(token)?;
        if expected != token.mac {
            return Err(CapError::BadMac);
        }
        for r in &token.rights {
            validate_right(r)?;
        }
        Ok(())
    }

    /// True if token is valid and contains `right`.
    pub fn allows(&self, token: &CapToken, right: &str) -> bool {
        self.verify(token).is_ok() && token.rights.contains(right)
    }

    fn mac_hex(&self, token: &CapToken) -> Result<String, CapError> {
        let payload = canonical_payload(token);
        let mut mac =
            HmacSha256::new_from_slice(&self.key).map_err(|e| CapError::Invalid(e.to_string()))?;
        mac.update(payload.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }
}

fn canonical_payload(token: &CapToken) -> String {
    // Stable order; exclude mac itself.
    let rights: Vec<&str> = token.rights.iter().map(|s| s.as_str()).collect();
    format!(
        "v1|id={}|parent={}|issued={}|expires={}|rights={}",
        token.id,
        token.parent.as_deref().unwrap_or(""),
        token.issued_at,
        token.expires_at.as_deref().unwrap_or(""),
        rights.join(",")
    )
}

fn validate_right(r: &str) -> Result<(), CapError> {
    match r {
        rights::LOCAL_MODEL
        | rights::PLOT_READ
        | rights::PLOT_WRITE
        | rights::NETWORK
        | rights::SHELL
        | rights::CLOUD_MODEL => Ok(()),
        _ => Err(CapError::UnknownRight(r.into())),
    }
}

/// Map charter flags to the full root right set.
pub fn rights_for_charter(
    allow_local_model: bool,
    allow_cloud_model: bool,
    allow_network: bool,
    allow_shell: bool,
) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    if allow_local_model {
        s.insert(rights::LOCAL_MODEL.into());
    }
    if allow_cloud_model {
        s.insert(rights::CLOUD_MODEL.into());
    }
    if allow_network {
        s.insert(rights::NETWORK.into());
    }
    if allow_shell {
        s.insert(rights::SHELL.into());
    }
    // Plot is always available under every pack today.
    s.insert(rights::PLOT_READ.into());
    s.insert(rights::PLOT_WRITE.into());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> CapAuthority {
        CapAuthority::new(vec![7u8; 32])
    }

    #[test]
    fn issue_and_verify() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert(rights::PLOT_READ.into());
        let t = a.issue(rights, Some(60)).unwrap();
        a.verify(&t).unwrap();
        assert!(a.allows(&t, rights::PLOT_READ));
        assert!(!a.allows(&t, rights::SHELL));
    }

    #[test]
    fn attenuate_subset() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert(rights::PLOT_READ.into());
        rights.insert(rights::PLOT_WRITE.into());
        let parent = a.issue(rights, None).unwrap();
        let mut keep = BTreeSet::new();
        keep.insert(rights::PLOT_READ.into());
        let child = a.attenuate(&parent, keep, None).unwrap();
        assert!(child.rights.contains(rights::PLOT_READ));
        assert!(!child.rights.contains(rights::PLOT_WRITE));
        assert_eq!(child.parent.as_deref(), Some(parent.id.as_str()));
    }

    #[test]
    fn attenuate_cannot_widen() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert(rights::PLOT_READ.into());
        let parent = a.issue(rights, None).unwrap();
        let mut keep = BTreeSet::new();
        keep.insert(rights::PLOT_READ.into());
        keep.insert(rights::SHELL.into());
        assert!(matches!(a.attenuate(&parent, keep, None), Err(CapError::Widen)));
    }

    #[test]
    fn unknown_right_rejected() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert("admin.all".into());
        assert!(matches!(
            a.issue(rights, None),
            Err(CapError::UnknownRight(_))
        ));
    }
}
