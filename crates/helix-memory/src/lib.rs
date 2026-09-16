use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Soft cap on preference text injected into an ask prompt (chars, approx tokens).
pub const PREFS_CONTEXT_CHAR_CAP: usize = 1500;

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("home directory not found")]
    NoHome,
    #[error("invalid preference name: {0}")]
    InvalidPrefName(String),
    #[error("preference not found: {0}")]
    PrefNotFound(String),
}

#[derive(Debug, Clone)]
pub struct HelixHome {
    pub root: PathBuf,
}

/// User verdict on an ask turn. Stored in episode JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Accept,
    Edit,
    Reject,
}

/// One recorded interaction under memory/episodes/.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub ts: String,
    pub query: String,
    pub reply: String,
    /// Present when the user edited the model reply before accepting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_reply: Option<String>,
    pub verdict: Verdict,
    pub pack: String,
    pub model_used: bool,
}

/// One preference: short rule stored as memory/prefs/<name>.md
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pref {
    pub name: String,
    pub text: String,
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
                "# Preferences\n\nOne short rule per file. Delete a file to forget it.\n\
Use `helix pref add NAME \"text\"` to add one.\n",
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

    pub fn prefs_dir(&self) -> PathBuf {
        self.root.join("memory/prefs")
    }

    /// Validate preference name: lowercase alphanumeric + hyphen, 1–64 chars.
    pub fn validate_pref_name(name: &str) -> Result<(), MemoryError> {
        if name.is_empty() || name.len() > 64 {
            return Err(MemoryError::InvalidPrefName(name.into()));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(MemoryError::InvalidPrefName(name.into()));
        }
        if name.starts_with('-') || name.ends_with('-') {
            return Err(MemoryError::InvalidPrefName(name.into()));
        }
        Ok(())
    }

    pub fn list_prefs(&self) -> Result<Vec<Pref>, MemoryError> {
        let dir = self.prefs_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(n) if n != "README" => n.to_string(),
                _ => continue,
            };
            let text = fs::read_to_string(&path).unwrap_or_default();
            out.push(Pref { name, text });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn add_pref(&self, name: &str, text: &str) -> Result<PathBuf, MemoryError> {
        Self::validate_pref_name(name)?;
        let dir = self.prefs_dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.md"));
        let body = text.trim();
        let body = if body.is_empty() {
            format!("# {name}\n")
        } else {
            format!("{body}\n")
        };
        fs::write(&path, body)?;
        Ok(path)
    }

    pub fn delete_pref(&self, name: &str) -> Result<(), MemoryError> {
        Self::validate_pref_name(name)?;
        let path = self.prefs_dir().join(format!("{name}.md"));
        if !path.exists() {
            return Err(MemoryError::PrefNotFound(name.into()));
        }
        fs::remove_file(path)?;
        Ok(())
    }

    /// Format preferences for the ask prompt, truncated to `max_chars`.
    pub fn prefs_context(&self, max_chars: usize) -> Result<String, MemoryError> {
        let prefs = self.list_prefs()?;
        if prefs.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::from("Preferences (user rules):\n");
        let mut used = out.len();
        for p in prefs {
            let line = format!("- {}: {}\n", p.name, p.text.trim().replace('\n', " "));
            if used + line.len() > max_chars {
                out.push_str("(further preferences truncated)\n");
                break;
            }
            out.push_str(&line);
            used += line.len();
        }
        Ok(out)
    }

    /// Keyword retrieval stub. Later this uses local embeddings.
    /// Prefs are injected separately via prefs_context so they are not double-counted.
    pub fn retrieve_context(&self, query: &str) -> Result<String, MemoryError> {
        let mut hits = Vec::new();
        // Skip memory/prefs — handled by prefs_context with a hard cap.
        for sub in ["episodes", "playbooks", "tools"] {
            collect_hits(&self.root.join("memory").join(sub), query, &mut hits)?;
        }
        if hits.is_empty() {
            return Ok("(no playbooks, tool notes, or episodes matched yet)\n".into());
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

    pub fn episodes_dir(&self) -> PathBuf {
        self.root.join("memory/episodes")
    }

    pub fn playbooks_dir(&self) -> PathBuf {
        self.root.join("memory/playbooks")
    }

    /// Persist an episode JSON under memory/episodes/.
    /// On Accept or Edit, may promote a short playbook after two similar successes.
    pub fn write_episode(&self, episode: &Episode) -> Result<PathBuf, MemoryError> {
        let dir = self.episodes_dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", episode.id));
        let body = serde_json::to_string_pretty(episode)?;
        fs::write(&path, body)?;
        if matches!(episode.verdict, Verdict::Accept | Verdict::Edit) {
            let _ = self.maybe_promote_playbook(episode);
        }
        Ok(path)
    }

    /// After two successful episodes with overlapping query tokens, write a playbook.
    fn maybe_promote_playbook(&self, episode: &Episode) -> Result<(), MemoryError> {
        let tokens = significant_tokens(&episode.query);
        if tokens.is_empty() {
            return Ok(());
        }
        let mut similar = 0u32;
        let dir = self.episodes_dir();
        if !dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let other: Episode = match serde_json::from_str(&text) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !matches!(other.verdict, Verdict::Accept | Verdict::Edit) {
                continue;
            }
            let other_tokens = significant_tokens(&other.query);
            let overlap = tokens
                .iter()
                .filter(|t| other_tokens.iter().any(|o| o == *t))
                .count();
            // Require at least half of the current query tokens in common.
            if overlap * 2 >= tokens.len() {
                similar += 1;
            }
        }
        // `similar` includes the episode we just wrote.
        if similar < 2 {
            return Ok(());
        }

        let playbooks = self.playbooks_dir();
        fs::create_dir_all(&playbooks)?;
        let slug = tokens
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join("-");
        let name = if slug.is_empty() {
            format!("auto-{}.md", &episode.id[..8.min(episode.id.len())])
        } else {
            format!("{slug}.md")
        };
        let path = playbooks.join(&name);
        if path.exists() {
            return Ok(());
        }
        let effective = episode
            .edited_reply
            .as_deref()
            .unwrap_or(episode.reply.as_str());
        let body = format!(
            "# Playbook: {slug}\n\n\
             Promoted after repeated successes on similar queries.\n\n\
             ## Example query\n\n{}\n\n\
             ## Working reply\n\n{}\n",
            episode.query.trim(),
            effective.trim()
        );
        fs::write(path, body)?;
        Ok(())
    }
}

fn significant_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_string())
        .collect()
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

/// Build a new episode with a unique id based on timestamp.
pub fn new_episode(
    query: &str,
    reply: &str,
    edited_reply: Option<String>,
    verdict: Verdict,
    pack: &str,
    model_used: bool,
) -> Episode {
    let ts = Utc::now();
    let id = format!("{}", ts.format("%Y%m%dT%H%M%S%.3fZ"));
    Episode {
        id,
        ts: ts.to_rfc3339(),
        query: query.to_string(),
        reply: reply.to_string(),
        edited_reply,
        verdict,
        pack: pack.to_string(),
        model_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> HelixHome {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        HelixHome { root }
    }

    #[test]
    fn write_episode_accept() {
        let home = temp_home();
        home.init("hearthside").unwrap();
        let ep = new_episode(
            "how do I list files",
            "use ls in the plot",
            None,
            Verdict::Accept,
            "hearthside",
            false,
        );
        let path = home.write_episode(&ep).unwrap();
        assert!(path.exists());
        let loaded: Episode = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.verdict, Verdict::Accept);
        assert_eq!(loaded.query, "how do I list files");
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn promote_after_two_similar() {
        let home = temp_home();
        home.init("hearthside").unwrap();
        let ep1 = new_episode(
            "how do I list files in plot",
            "Stay inside the plot workspace.",
            None,
            Verdict::Accept,
            "hearthside",
            false,
        );
        home.write_episode(&ep1).unwrap();
        // Force unique id for second episode
        let mut ep2 = new_episode(
            "list files inside the plot workspace",
            "Only touch files under plots/default.",
            None,
            Verdict::Accept,
            "hearthside",
            false,
        );
        ep2.id = format!("{}-b", ep2.id);
        home.write_episode(&ep2).unwrap();
        let playbooks: Vec<_> = fs::read_dir(home.playbooks_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .collect();
        assert!(
            !playbooks.is_empty(),
            "expected a playbook after two similar accepts"
        );
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn pref_add_list_delete() {
        let home = temp_home();
        home.init("hearthside").unwrap();
        let path = home
            .add_pref("tone", "Prefer concise bullet replies.")
            .unwrap();
        assert!(path.exists());
        let list = home.list_prefs().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "tone");
        assert!(list[0].text.contains("concise"));
        home.delete_pref("tone").unwrap();
        assert!(home.list_prefs().unwrap().is_empty());
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn pref_context_capped() {
        let home = temp_home();
        home.init("hearthside").unwrap();
        home.add_pref("a", "short rule one").unwrap();
        home.add_pref("b", "short rule two").unwrap();
        let ctx = home.prefs_context(80).unwrap();
        assert!(ctx.contains("Preferences"));
        // With a tiny cap, second pref may be truncated
        assert!(ctx.len() <= 120);
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn invalid_pref_name() {
        assert!(HelixHome::validate_pref_name("Bad Name").is_err());
        assert!(HelixHome::validate_pref_name("").is_err());
        assert!(HelixHome::validate_pref_name("ok-name-1").is_ok());
    }
}
