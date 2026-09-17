//! Reliquary: named secret references.
//!
//! Secrets are stored as sealed entries under `~/Helix/reliquary/`.
//! Values never enter model context. Unwrap is only for future adapters
//! at a Switch boundary; the keychain backend is a no-op stub in this slice.
//!
//! CLI: `helix secrets list|add|revoke`. List and revoke never print values.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use helix_memory::HelixHome;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReliquaryError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid secret name: {0}")]
    InvalidName(String),
    #[error("secret not found: {0}")]
    NotFound(String),
    #[error("secret already exists: {0}")]
    AlreadyExists(String),
    #[error("keychain backend is a stub; unwrap not available yet")]
    KeychainStub,
    #[error("empty secret value")]
    EmptyValue,
}

/// Backend that holds the sealed material. Keychain is stubbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    /// Local sealed file under reliquary/ (0600). Values never listed.
    LocalSealed,
    /// Future OS keychain (macOS Keychain / DPAPI / libsecret). Unwrap is no-op.
    KeychainStub,
}

/// Metadata for one named secret reference. Never includes the value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMeta {
    pub name: String,
    pub backend: Backend,
    pub created_at: String,
    /// Opaque reference id for the sealed blob / keychain account.
    pub ref_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Catalog {
    entries: BTreeMap<String, SecretMeta>,
}

/// Sealed values map: name -> value. Loaded only for store/unwrap ops.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SealedStore {
    /// name -> value (never serialized into model prompts)
    values: BTreeMap<String, String>,
}

pub struct Reliquary {
    home: HelixHome,
}

impl Reliquary {
    pub fn open(home: HelixHome) -> Result<Self, ReliquaryError> {
        let r = Self { home };
        r.ensure_dirs()?;
        Ok(r)
    }

    pub fn from_resolved() -> Result<Self, ReliquaryError> {
        let home = HelixHome::resolve().map_err(|e| {
            ReliquaryError::Io(io::Error::new(io::ErrorKind::NotFound, e.to_string()))
        })?;
        Self::open(home)
    }

    fn reliquary_dir(&self) -> PathBuf {
        self.home.root.join("reliquary")
    }

    fn catalog_path(&self) -> PathBuf {
        self.reliquary_dir().join("catalog.json")
    }

    fn sealed_path(&self) -> PathBuf {
        self.reliquary_dir().join("sealed.json")
    }

    fn ensure_dirs(&self) -> Result<(), ReliquaryError> {
        fs::create_dir_all(self.reliquary_dir())?;
        // Restrict permissions on Unix (best-effort).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.reliquary_dir(), fs::Permissions::from_mode(0o700));
        }
        if !self.catalog_path().exists() {
            self.write_catalog(&Catalog::default())?;
        }
        if !self.sealed_path().exists() {
            self.write_sealed(&SealedStore::default())?;
        }
        Ok(())
    }

    fn read_catalog(&self) -> Result<Catalog, ReliquaryError> {
        let path = self.catalog_path();
        if !path.exists() {
            return Ok(Catalog::default());
        }
        let text = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn write_catalog(&self, cat: &Catalog) -> Result<(), ReliquaryError> {
        let path = self.catalog_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(cat)?;
        fs::write(&path, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    fn read_sealed(&self) -> Result<SealedStore, ReliquaryError> {
        let path = self.sealed_path();
        if !path.exists() {
            return Ok(SealedStore::default());
        }
        let text = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn write_sealed(&self, store: &SealedStore) -> Result<(), ReliquaryError> {
        let path = self.sealed_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(store)?;
        fs::write(&path, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Validate name: lowercase alphanumeric + hyphen, 1–64 chars.
    pub fn validate_name(name: &str) -> Result<(), ReliquaryError> {
        if name.is_empty() || name.len() > 64 {
            return Err(ReliquaryError::InvalidName(name.into()));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(ReliquaryError::InvalidName(name.into()));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(ReliquaryError::InvalidName(name.into()));
        }
        Ok(())
    }

    /// List metadata only. Never returns secret values.
    pub fn list(&self) -> Result<Vec<SecretMeta>, ReliquaryError> {
        let cat = self.read_catalog()?;
        Ok(cat.entries.into_values().collect())
    }

    /// Add a named secret. Value is sealed locally; catalog holds only a reference.
    /// Keychain backend is recorded as stub for future migration.
    pub fn add(
        &self,
        name: &str,
        value: &str,
        prefer_keychain: bool,
    ) -> Result<SecretMeta, ReliquaryError> {
        Self::validate_name(name)?;
        if value.is_empty() {
            return Err(ReliquaryError::EmptyValue);
        }
        let mut cat = self.read_catalog()?;
        if cat.entries.contains_key(name) {
            return Err(ReliquaryError::AlreadyExists(name.into()));
        }

        let backend = if prefer_keychain {
            // Stub: we still seal locally and mark as keychain-stub for visibility.
            let _ = keychain_store_stub(name, value);
            Backend::KeychainStub
        } else {
            Backend::LocalSealed
        };

        let ref_id = format!("rel-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let meta = SecretMeta {
            name: name.to_string(),
            backend,
            created_at: Utc::now().to_rfc3339(),
            ref_id: ref_id.clone(),
        };

        // Always keep a local sealed copy in this slice so revoke works offline.
        let mut sealed = self.read_sealed()?;
        sealed.values.insert(name.to_string(), value.to_string());
        self.write_sealed(&sealed)?;

        cat.entries.insert(name.to_string(), meta.clone());
        self.write_catalog(&cat)?;
        Ok(meta)
    }

    /// Revoke (delete) a named secret reference and its sealed material.
    pub fn revoke(&self, name: &str) -> Result<(), ReliquaryError> {
        Self::validate_name(name)?;
        let mut cat = self.read_catalog()?;
        if cat.entries.remove(name).is_none() {
            return Err(ReliquaryError::NotFound(name.into()));
        }
        let mut sealed = self.read_sealed()?;
        sealed.values.remove(name);
        self.write_sealed(&sealed)?;
        self.write_catalog(&cat)?;
        let _ = keychain_delete_stub(name);
        Ok(())
    }

    /// Unwrap for adapters only. In this slice, keychain is stub; local-sealed returns value.
    /// Callers must never put the result into model context.
    pub fn unwrap(&self, name: &str) -> Result<String, ReliquaryError> {
        Self::validate_name(name)?;
        let cat = self.read_catalog()?;
        let meta = cat
            .entries
            .get(name)
            .ok_or_else(|| ReliquaryError::NotFound(name.into()))?;
        match meta.backend {
            Backend::KeychainStub => {
                // Keychain unwrap is a no-op stub in this slice. Prefer local
                // sealed copy so adapters can still be developed offline.
                match keychain_unwrap_stub(name) {
                    Ok(v) => Ok(v),
                    Err(ReliquaryError::KeychainStub) => {
                        let sealed = self.read_sealed()?;
                        sealed
                            .values
                            .get(name)
                            .cloned()
                            .ok_or_else(|| ReliquaryError::NotFound(name.into()))
                    }
                    Err(e) => Err(e),
                }
            }
            Backend::LocalSealed => {
                let sealed = self.read_sealed()?;
                sealed
                    .values
                    .get(name)
                    .cloned()
                    .ok_or_else(|| ReliquaryError::NotFound(name.into()))
            }
        }
    }
}

/// OS keychain store stub (macOS Keychain / DPAPI / libsecret). No-op success.
fn keychain_store_stub(_name: &str, _value: &str) -> Result<(), ReliquaryError> {
    Ok(())
}

fn keychain_delete_stub(_name: &str) -> Result<(), ReliquaryError> {
    Ok(())
}

fn keychain_unwrap_stub(_name: &str) -> Result<String, ReliquaryError> {
    Err(ReliquaryError::KeychainStub)
}

/// Ensure reliquary dir exists as part of home init (called from memory or CLI).
pub fn ensure_reliquary_layout(root: &Path) -> io::Result<()> {
    let dir = root.join("reliquary");
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let catalog = dir.join("catalog.json");
    if !catalog.exists() {
        let body = serde_json::to_string_pretty(&Catalog::default()).unwrap_or_else(|_| "{}".into());
        fs::write(&catalog, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&catalog, fs::Permissions::from_mode(0o600));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_reliquary() -> Reliquary {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-rel-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init("hearthside").unwrap();
        Reliquary::open(home).unwrap()
    }

    #[test]
    fn add_list_revoke() {
        let r = temp_reliquary();
        let meta = r.add("api-token", "s3cr3t-value", false).unwrap();
        assert_eq!(meta.name, "api-token");
        assert_eq!(meta.backend, Backend::LocalSealed);
        let list = r.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "api-token");
        // list must not expose value — only meta is returned
        let unwrapped = r.unwrap("api-token").unwrap();
        assert_eq!(unwrapped, "s3cr3t-value");
        r.revoke("api-token").unwrap();
        assert!(r.list().unwrap().is_empty());
        assert!(r.unwrap("api-token").is_err());
        let _ = fs::remove_dir_all(&r.home.root);
    }

    #[test]
    fn reject_invalid_name() {
        assert!(Reliquary::validate_name("Bad").is_err());
        assert!(Reliquary::validate_name("ok-name").is_ok());
    }

    #[test]
    fn no_duplicate() {
        let r = temp_reliquary();
        r.add("x", "v1", false).unwrap();
        assert!(matches!(
            r.add("x", "v2", false),
            Err(ReliquaryError::AlreadyExists(_))
        ));
        let _ = fs::remove_dir_all(&r.home.root);
    }

    #[test]
    fn keychain_stub_flag() {
        let r = temp_reliquary();
        let meta = r.add("kc-item", "hidden", true).unwrap();
        assert_eq!(meta.backend, Backend::KeychainStub);
        // Still unwraps via local sealed fallback in this slice
        assert_eq!(r.unwrap("kc-item").unwrap(), "hidden");
        let _ = fs::remove_dir_all(&r.home.root);
    }
}
