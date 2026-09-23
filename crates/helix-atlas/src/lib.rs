//! Atlas: digest-pinned tool catalog for Helix.
//!
//! Tools are recorded under `~/Helix/atlas/pins.json` by **content hash**
//! (SHA-256), not by path tag. Unsigned / unpinned local Wasm modules are
//! treated as **untrusted** and receive a narrower Hands charter (default
//! fuel cap) when run through `helix hands run`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use helix_memory::HelixHome;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Default instruction fuel for tools that are not digest-pinned (or whose
/// on-disk bytes no longer match the pin). Trusted pins may omit fuel.
pub const UNTRUSTED_FUEL_CAP: u64 = 50_000_000;

#[derive(Debug, Error)]
pub enum AtlasError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid tool name: {0}")]
    InvalidName(String),
    #[error("pin not found: {0}")]
    NotFound(String),
    #[error("pin digest mismatch for {name}: expected {expected}, got {actual}")]
    DigestMismatch {
        name: String,
        expected: String,
        actual: String,
    },
}

/// One pinned tool entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pin {
    /// Stable short name (lowercase, digits, hyphens).
    pub name: String,
    /// Path recorded at pin time (informational; trust is the digest).
    pub path: String,
    /// Lowercase hex SHA-256 of the file bytes at pin time.
    pub sha256: String,
    /// Explicit trust bit. Always true for entries written by `atlas pin`.
    #[serde(default = "default_true")]
    pub trusted: bool,
    /// When the pin was recorded (RFC3339).
    pub pinned_at: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PinFile {
    #[serde(default)]
    pub tools: Vec<Pin>,
}

/// Trust decision for a Wasm module about to run under Hands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// Digest matches a pin; full fuel allowed (unless user set one).
    Trusted { name: String, sha256: String },
    /// No pin, or bytes drifted — apply [`UNTRUSTED_FUEL_CAP`].
    Untrusted {
        reason: String,
        sha256: String,
        fuel_cap: u64,
    },
}

/// Atlas catalog bound to a Helix home.
#[derive(Debug, Clone)]
pub struct Atlas {
    home: HelixHome,
}

impl Atlas {
    pub fn open(home: HelixHome) -> Self {
        Self { home }
    }

    pub fn pins_path(&self) -> PathBuf {
        self.home.root.join("atlas/pins.json")
    }

    /// Ensure atlas dir + empty pins.json exist.
    pub fn ensure(&self) -> Result<(), AtlasError> {
        let dir = self.home.root.join("atlas");
        fs::create_dir_all(&dir)?;
        let path = self.pins_path();
        if !path.exists() {
            let empty = PinFile::default();
            fs::write(&path, serde_json::to_string_pretty(&empty)? + "\n")?;
        }
        Ok(())
    }

    pub fn load(&self) -> Result<PinFile, AtlasError> {
        self.ensure()?;
        let path = self.pins_path();
        let text = fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            return Ok(PinFile::default());
        }
        Ok(serde_json::from_str(&text)?)
    }

    fn save(&self, file: &PinFile) -> Result<(), AtlasError> {
        self.ensure()?;
        let path = self.pins_path();
        fs::write(&path, serde_json::to_string_pretty(file)? + "\n")?;
        Ok(())
    }

    /// Validate tool name: lowercase alphanumeric + hyphen, 1–64 chars.
    pub fn validate_name(name: &str) -> Result<(), AtlasError> {
        if name.is_empty() || name.len() > 64 {
            return Err(AtlasError::InvalidName(name.into()));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(AtlasError::InvalidName(name.into()));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(AtlasError::InvalidName(name.into()));
        }
        Ok(())
    }

    /// SHA-256 hex of file contents.
    pub fn hash_file(path: &Path) -> Result<String, AtlasError> {
        let bytes = fs::read(path)?;
        Ok(hash_bytes(&bytes))
    }

    pub fn hash_bytes(bytes: &[u8]) -> String {
        hash_bytes(bytes)
    }

    /// List all pins.
    pub fn list(&self) -> Result<Vec<Pin>, AtlasError> {
        let mut tools = self.load()?.tools;
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tools)
    }

    /// Pin a tool by path. Overwrites an existing pin of the same name.
    pub fn pin(&self, name: &str, path: &Path) -> Result<Pin, AtlasError> {
        Self::validate_name(name)?;
        let abs = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf());
        let sha256 = Self::hash_file(path)?;
        let entry = Pin {
            name: name.to_string(),
            path: abs.display().to_string(),
            sha256,
            trusted: true,
            pinned_at: Utc::now().to_rfc3339(),
        };
        let mut file = self.load()?;
        if let Some(i) = file.tools.iter().position(|t| t.name == name) {
            file.tools[i] = entry.clone();
        } else {
            file.tools.push(entry.clone());
        }
        self.save(&file)?;
        Ok(entry)
    }

    /// Remove a pin by name.
    pub fn unpin(&self, name: &str) -> Result<(), AtlasError> {
        Self::validate_name(name)?;
        let mut file = self.load()?;
        let before = file.tools.len();
        file.tools.retain(|t| t.name != name);
        if file.tools.len() == before {
            return Err(AtlasError::NotFound(name.into()));
        }
        self.save(&file)?;
        Ok(())
    }

    /// Verify that a named pin still matches on-disk bytes at the recorded path.
    pub fn verify_name(&self, name: &str) -> Result<Pin, AtlasError> {
        Self::validate_name(name)?;
        let file = self.load()?;
        let pin = file
            .tools
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .ok_or_else(|| AtlasError::NotFound(name.into()))?;
        let actual = Self::hash_file(Path::new(&pin.path))?;
        if actual != pin.sha256 {
            return Err(AtlasError::DigestMismatch {
                name: name.into(),
                expected: pin.sha256,
                actual,
            });
        }
        Ok(pin)
    }

    /// Decide trust for arbitrary Wasm bytes (and optional path for messages).
    pub fn classify(&self, bytes: &[u8], path_hint: Option<&str>) -> Result<Trust, AtlasError> {
        let sha256 = hash_bytes(bytes);
        let file = self.load()?;
        if let Some(pin) = file.tools.iter().find(|t| t.sha256 == sha256 && t.trusted) {
            return Ok(Trust::Trusted {
                name: pin.name.clone(),
                sha256,
            });
        }
        // Same path but different digest → drift.
        if let Some(hint) = path_hint {
            let hint_path = Path::new(hint);
            let abs = hint_path
                .canonicalize()
                .unwrap_or_else(|_| hint_path.to_path_buf());
            let abs_s = abs.display().to_string();
            if let Some(pin) = file.tools.iter().find(|t| t.path == abs_s || t.path == hint) {
                if pin.sha256 != sha256 {
                    return Ok(Trust::Untrusted {
                        reason: format!(
                            "digest drift for pin '{}': expected {}, got {}",
                            pin.name, pin.sha256, sha256
                        ),
                        sha256,
                        fuel_cap: UNTRUSTED_FUEL_CAP,
                    });
                }
            }
        }
        Ok(Trust::Untrusted {
            reason: "unsigned local tool (not in atlas/pins.json)".into(),
            sha256,
            fuel_cap: UNTRUSTED_FUEL_CAP,
        })
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_atlas() -> Atlas {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-atlas-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init("hearthside").unwrap();
        Atlas::open(home)
    }

    #[test]
    fn pin_list_unpin_verify() {
        let atlas = temp_atlas();
        let dir = atlas.home.root.join("tools");
        fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("demo.wasm");
        fs::write(&wasm, b"\0asm\x01\0\0\0").unwrap();

        let pin = atlas.pin("demo", &wasm).unwrap();
        assert!(pin.trusted);
        assert_eq!(pin.sha256.len(), 64);

        let list = atlas.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "demo");

        atlas.verify_name("demo").unwrap();

        // Drift
        fs::write(&wasm, b"\0asm\x01\0\0\0changed").unwrap();
        let err = atlas.verify_name("demo").unwrap_err();
        assert!(matches!(err, AtlasError::DigestMismatch { .. }));

        atlas.unpin("demo").unwrap();
        assert!(atlas.list().unwrap().is_empty());
        let _ = fs::remove_dir_all(&atlas.home.root);
    }

    #[test]
    fn classify_trusted_and_untrusted() {
        let atlas = temp_atlas();
        let dir = atlas.home.root.join("tools");
        fs::create_dir_all(&dir).unwrap();
        let wasm = dir.join("ok.wasm");
        let bytes = b"\0asm\x01\0\0\0trusted";
        fs::write(&wasm, bytes).unwrap();
        atlas.pin("ok", &wasm).unwrap();

        match atlas.classify(bytes, Some(wasm.to_str().unwrap())).unwrap() {
            Trust::Trusted { name, .. } => assert_eq!(name, "ok"),
            other => panic!("expected trusted, got {other:?}"),
        }

        match atlas.classify(b"other", None).unwrap() {
            Trust::Untrusted { fuel_cap, .. } => assert_eq!(fuel_cap, UNTRUSTED_FUEL_CAP),
            other => panic!("expected untrusted, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&atlas.home.root);
    }

    #[test]
    fn invalid_name() {
        assert!(Atlas::validate_name("Bad").is_err());
        assert!(Atlas::validate_name("ok-1").is_ok());
    }
}
