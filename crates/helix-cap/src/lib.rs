//! Shrink-only capability tokens for Helix.
//!
//! Tokens are issued by helixd with a session MAC key. A holder may only
//! *attenuate* (remove rights); subcommands cannot widen the rights set.
//! This is the product equivalent of Biscuit attenuation without pulling the
//! full biscuit-auth stack into the default path.

use std::collections::BTreeSet;

use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// Known action keys that map to charter flags.
pub mod rights {
    pub const LOCAL_MODEL: &str = "local.model";
    pub const PLOT_READ: &str = "plot.read";
    pub const PLOT_WRITE: &str = "plot.write";
    pub const NETWORK: &str = "network.adapter";
    pub const SHELL: &str = "shell";
    pub const CLOUD_MODEL: &str = "cloud.model";
}

#[derive(Debug, Error)]
pub enum CapError {
    #[error("invalid token: {0}")]
    Invalid(String),
    #[error("attenuation would widen rights")]
    WidenForbidden,
    #[error("token expired")]
    Expired,
    #[error("mac verification failed")]
    BadMac,
    #[error("unknown right: {0}")]
    UnknownRight(String),
    #[error("empty rights")]
    EmptyRights,
}

/// Public token body + MAC. Values never include secrets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapToken {
    pub id: String,
    /// Parent token id when this was attenuated from another token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub issued_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    /// Sorted unique action keys. Attenuation may only remove entries.
    pub rights: BTreeSet<String>,
    /// Hex-encoded HMAC-SHA256 over the canonical payload (without `mac`).
    pub mac: String,
}

/// Authority that can issue and verify tokens for one daemon process.
#[derive(Clone)]
pub struct CapAuthority {
    key: [u8; 32],
}

impl CapAuthority {
    /// Fresh random key for this process (tokens die on daemon restart).
    pub fn new_random() -> Self {
        let mut key = [0u8; 32];
        // Prefer OS randomness; fall back to time-based mix if unavailable.
        if getrandom_fill(&mut key).is_err() {
            let t = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;
            for (i, b) in key.iter_mut().enumerate() {
                *b = ((t.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (i % 8)) ^ (i as u64)) as u8;
            }
        }
        Self { key }
    }

    /// Deterministic key for tests.
    pub fn from_bytes(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Issue a root token with the given rights (already filtered by charter).
    pub fn issue(
        &self,
        rights: BTreeSet<String>,
        ttl_secs: Option<u64>,
    ) -> Result<CapToken, CapError> {
        if rights.is_empty() {
            return Err(CapError::EmptyRights);
        }
        for r in &rights {
            validate_right(r)?;
        }
        let now = Utc::now();
        let expires_at = ttl_secs.map(|s| (now + Duration::seconds(s as i64)).to_rfc3339());
        let id = format!("t-{}", now.format("%Y%m%dT%H%M%S%.3fZ"));
        let mut token = CapToken {
            id,
            parent: None,
            issued_at: now.to_rfc3339(),
            expires_at,
            rights,
            mac: String::new(),
        };
        token.mac = self.mac_hex(&token)?;
        Ok(token)
    }

    /// Produce a child token whose rights are a subset of `parent`.
    /// Subcommands cannot widen rights.
    pub fn attenuate(
        &self,
        parent: &CapToken,
        keep: BTreeSet<String>,
        ttl_secs: Option<u64>,
    ) -> Result<CapToken, CapError> {
        self.verify(parent)?;
        if !keep.is_subset(&parent.rights) {
            return Err(CapError::WidenForbidden);
        }
        if keep.is_empty() {
            return Err(CapError::EmptyRights);
        }
        for r in &keep {
            validate_right(r)?;
        }
        let now = Utc::now();
        // Child cannot outlive parent.
        let parent_exp = parent
            .expires_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc));
        let mut expires_at = ttl_secs.map(|s| (now + Duration::seconds(s as i64)).to_rfc3339());
        if let Some(pe) = parent_exp {
            let child_exp = expires_at
                .as_ref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(pe);
            let capped = if child_exp > pe { pe } else { child_exp };
            expires_at = Some(capped.to_rfc3339());
        }
        let id = format!("t-{}", now.format("%Y%m%dT%H%M%S%.3fZ"));
        let mut token = CapToken {
            id,
            parent: Some(parent.id.clone()),
            issued_at: now.to_rfc3339(),
            expires_at,
            rights: keep,
            mac: String::new(),
        };
        token.mac = self.mac_hex(&token)?;
        Ok(token)
    }

    /// Verify MAC and expiry. Does not check parent chain (stateless).
    pub fn verify(&self, token: &CapToken) -> Result<(), CapError> {
        if token.rights.is_empty() {
            return Err(CapError::EmptyRights);
        }
        for r in &token.rights {
            validate_right(r)?;
        }
        let expected = self.mac_hex(token)?;
        if !constant_time_eq(expected.as_bytes(), token.mac.as_bytes()) {
            return Err(CapError::BadMac);
        }
        if let Some(ref exp) = token.expires_at {
            let exp_dt = chrono::DateTime::parse_from_rfc3339(exp)
                .map_err(|e| CapError::Invalid(e.to_string()))?
                .with_timezone(&Utc);
            if Utc::now() > exp_dt {
                return Err(CapError::Expired);
            }
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
    format!
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

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn getrandom_fill(buf: &mut [u8]) -> Result<(), ()> {
    // Minimal OS randomness without extra deps: /dev/urandom on Unix.
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f = std::fs::File::open("/dev/urandom").map_err(|_| ())?;
        f.read_exact(buf).map_err(|_| ())?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = buf;
        Err(())
    }
}

/// Build the maximum rights set allowed by a charter pack.
pub fn rights_for_charter(
    allow_local_model: bool,
    allow_cloud_model: bool,
    allow_network_adapters: bool,
    allow_shell: bool,
) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    // Plot is always available in current packs.
    s.insert(rights::PLOT_READ.into());
    s.insert(rights::PLOT_WRITE.into());
    if allow_local_model {
        s.insert(rights::LOCAL_MODEL.into());
    }
    if allow_cloud_model {
        s.insert(rights::CLOUD_MODEL.into());
    }
    if allow_network_adapters {
        s.insert(rights::NETWORK.into());
    }
    if allow_shell {
        s.insert(rights::SHELL.into());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth() -> CapAuthority {
        CapAuthority::from_bytes([7u8; 32])
    }

    #[test]
    fn issue_and_verify() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert(rights::PLOT_READ.into());
        rights.insert(rights::LOCAL_MODEL.into());
        let t = a.issue(rights.clone(), Some(3600)).unwrap();
        a.verify(&t).unwrap();
        assert!(a.allows(&t, rights::PLOT_READ));
        assert!(!a.allows(&t, rights::SHELL));
    }

    #[test]
    fn attenuate_shrinks_only() {
        let a = auth();
        let mut full = BTreeSet::new();
        full.insert(rights::PLOT_READ.into());
        full.insert(rights::PLOT_WRITE.into());
        full.insert(rights::LOCAL_MODEL.into());
        let root = a.issue(full, None).unwrap();

        let mut keep = BTreeSet::new();
        keep.insert(rights::PLOT_READ.into());
        let child = a.attenuate(&root, keep, None).unwrap();
        assert_eq!(child.parent.as_deref(), Some(root.id.as_str()));
        assert!(child.rights.contains(rights::PLOT_READ));
        assert!(!child.rights.contains(rights::PLOT_WRITE));
        a.verify(&child).unwrap();

        // Cannot widen.
        let mut widen = child.rights.clone();
        widen.insert(rights::SHELL.into());
        assert!(matches!(
            a.attenuate(&child, widen, None),
            Err(CapError::WidenForbidden)
        ));
    }

    #[test]
    fn tampered_mac_fails() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert(rights::PLOT_READ.into());
        let mut t = a.issue(rights, None).unwrap();
        t.mac = "00".repeat(32);
        assert!(matches!(a.verify(&t), Err(CapError::BadMac)));
    }

    #[test]
    fn unknown_right_rejected() {
        let a = auth();
        let mut rights = BTreeSet::new();
        rights.insert("admin.all".into());
        assert!(matches!(a.issue(rights, None), Err(CapError::UnknownRight(_))));
    }
}
