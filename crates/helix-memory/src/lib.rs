use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("home directory not found")]
    NoHome,
}

#[derive(Debug, Clone)]
pub struct HelixHome {
    pub root: PathBuf,
}

impl HelixHome {
    pub fn resolve() -> Result<Self, MemoryError> {
        if let Ok(p) = std::env::var("HELIX_HOME") {
            return Ok(Self {
                root: PathBuf::from(p),
            });
        }
        let home = dirs::home_dir().ok_or(MemoryError::NoHome)?;
        Ok(Self {
            root: home.join("Helix"),
        })
    }

    pub fn init(&self, pack: &str) -> Result<(), MemoryError> {
        for rel in [
            "config",
            "plots/default",
            "memory/episodes",
            "memory/playbooks",
            "memory/tools",
            "memory/prefs",
            "atlas",
            "chronicle",
        ] {
            fs::create_dir_all(self.root.join(rel))?;
        }

        let charter = self.root.join("config/charter.toml");
        if !charter.exists() {
            fs::write(&charter, format!("pack = \"{pack}\"\n"))?;
        }

        let pins = self.root.join("atlas/pins.json");
        if !pins.exists() {
            fs::write(&pins, "{\n  \"tools\": []\n}\n")?;
        }

        let log = self.root.join("chronicle/log.jsonl");
        if !log.exists() {
            fs::File::create(&log)?;
        }

        let readme = self.root.join("plots/default/README.md");
        if !readme.exists() {
            fs::write(
                readme,
                "# Default plot\n\nThis is the only workspace Hands may see by default.\n",
            )?;
        }

        let prefs = self.root.join("memory/prefs/README.md");
        if !prefs.exists() {
            fs::write(
                prefs,
                "# Preferences\n\nOne short rule per file. Delete a file to forget it.\n",
            )?;
        }

        Ok(())
    }

    pub fn charter_path(&self) -> PathBuf {
        self.root.join("config/charter.toml")
    }

    pub fn write_pack(&self, pack: &str) -> Result<(), MemoryError> {
        fs::create_dir_all(self.root.join("config"))?;
        fs::write(self.charter_path(), format!("pack = \"{pack}\"\n"))?;
        Ok(())
    }

    pub fn read_pack(&self) -> Result<String, MemoryError> {
        let text = fs::read_to_string(self.charter_path())
            .unwrap_or_else(|_| "pack = \"hearthside\"\n".into());
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pack") {
                let rest = rest.trim().trim_start_matches('=').trim();
                let pack = rest.trim_matches('"').trim_matches('\'').to_string();
                if !pack.is_empty() {
                    return Ok(pack);
                }
            }
        }
        Ok("hearthside".into())
    }

    /// Keyword retrieval stub. Later this uses local embeddings.
    pub fn retrieve_context(&self, query: &str) -> Result<String, MemoryError> {
        let mut hits = Vec::new();
        collect_hits(&self.root.join("memory"), query, &mut hits)?;
        if hits.is_empty() {
            return Ok("(no playbooks, tool notes, or preferences matched yet)\n".into());
        }
        Ok(hits.join("\n---\n"))
    }

    pub fn append_chronicle(&self, line: &str) -> Result<(), MemoryError> {
        let path = self.root.join("chronicle/log.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(f, "{line}")?;
        Ok(())
    }
}

fn collect_hits(dir: &Path, query: &str, out: &mut Vec<String>) -> io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let q = query.to_lowercase();
    let tokens: Vec<&str> = q.split_whitespace().filter(|t| t.len() > 2).collect();
    visit(dir, &tokens, out)
}

fn visit(dir: &Path, tokens: &[&str], out: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit(&path, tokens, out)?;
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) == Some("md")
            || path.extension().and_then(|s| s.to_str()) == Some("json")
        {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let lower = text.to_lowercase();
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let hit =
                tokens.is_empty() || tokens.iter().any(|t| lower.contains(t) || name.contains(t));
            if hit && path.file_name().and_then(|s| s.to_str()) != Some("README.md") {
                out.push(format!(
                    "{}:\n{}",
                    path.display(),
                    text.chars().take(800).collect::<String>()
                ));
            }
        }
        if out.len() >= 6 {
            break;
        }
    }
    Ok(())
}
