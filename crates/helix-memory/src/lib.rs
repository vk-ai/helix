use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    #[error("plot not found: {0}")]
    PlotNotFound(String),
    #[error("commit not found: {0}")]
    CommitNotFound(String),
    #[error("ambiguous commit prefix: {0}")]
    AmbiguousCommit(String),
    #[error("invalid plot name: {0}")]
    InvalidPlotName(String),
    #[error("invalid path under plot: {0}")]
    InvalidPlotPath(String),
    #[error("path not found under plot: {0}")]
    PlotPathNotFound(String),
    #[error("path is a directory: {0}")]
    PlotPathIsDir(String),
    #[error("path is not a directory: {0}")]
    PlotPathNotDir(String),
}

#[derive(Debug, Clone)]
pub struct HelixHome {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: String,
    pub created_at: String,
    pub query: String,
    pub reply: String,
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitMeta {
    pub id: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub file_count: usize,
}

/// A single entry from a plot-scoped directory listing.
#[derive(Debug, Clone)]
pub struct PlotEntry {
    /// Path relative to the plot root (forward slashes).
    pub path: String,
    pub is_dir: bool,
}

impl HelixHome {
    pub fn resolve() -> Result<Self, MemoryError> {
        if let Ok(h) = std::env::var("HELIX_HOME") {
            return Ok(Self { root: PathBuf::from(h) });
        }
        let home = dirs::home_dir().ok_or(MemoryError::NoHome)?;
        Ok(Self { root: home.join("Helix") })
    }

    pub fn init(&self, pack: &str) -> Result<(), MemoryError> {
        let dirs = [
            self.root.join("config"),
            self.root.join("plots").join("default"),
            self.root.join("memory").join("episodes"),
            self.root.join("memory").join("playbooks"),
            self.root.join("memory").join("tools"),
            self.root.join("memory").join("prefs"),
            self.root.join("reliquary"),
            self.root.join("atlas"),
            self.root.join("chronicle"),
        ];
        for d in &dirs {
            fs::create_dir_all(d)?;
        }
        let charter = self.root.join("config/charter.toml");
        if !charter.exists() {
            self.write_pack(pack)?;
        }
        let pins = self.root.join("atlas/pins.json");
        if !pins.exists() {
            fs::write(&pins, "{\n  \"tools\": []\n}\n")?;
        }
        let chronicle = self.root.join("chronicle/log.jsonl");
        if !chronicle.exists() {
            fs::File::create(&chronicle)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.root.join("reliquary"), fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    pub fn pack_path(&self) -> PathBuf {
        self.root.join("config/charter.toml")
    }

    pub fn read_pack(&self) -> Result<String, MemoryError> {
        let path = self.pack_path();
        if !path.exists() {
            return Ok("hearthside".into());
        }
        let text = fs::read_to_string(&path)?;
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("pack") {
                let rest = rest.trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let v = rest.trim().trim_matches('"').trim();
                    if !v.is_empty() {
                        return Ok(v.to_string());
                    }
                }
            }
        }
        Ok("hearthside".into())
    }

    pub fn write_pack(&self, pack: &str) -> Result<(), MemoryError> {
        fs::create_dir_all(self.root.join("config"))?;
        fs::write(self.pack_path(), format!("pack = \"{pack}\"\n"))?;
        Ok(())
    }

    pub fn append_chronicle(&self, line: &str) -> Result<(), MemoryError> {
        let path = self.root.join("chronicle/log.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    fn prefs_dir(&self) -> PathBuf {
        self.root.join("memory/prefs")
    }

    fn validate_pref_name(name: &str) -> Result<(), MemoryError> {
        if name.is_empty() || name.len() > 64 {
            return Err(MemoryError::InvalidPrefName(name.into()));
        }
        if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(MemoryError::InvalidPrefName(name.into()));
        }
        Ok(())
    }

    pub fn pref_add(&self, name: &str, body: &str) -> Result<(), MemoryError> {
        Self::validate_pref_name(name)?;
        let dir = self.prefs_dir();
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(format!("{name}.md")), body.trim())?;
        Ok(())
    }

    pub fn pref_list(&self) -> Result<Vec<(String, String)>, MemoryError> {
        let dir = self.prefs_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let body = fs::read_to_string(&path).unwrap_or_default();
            out.push((name, body));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub fn pref_delete(&self, name: &str) -> Result<(), MemoryError> {
        let path = self.prefs_dir().join(format!("{name}.md"));
        Self::validate_pref_name(name)?;
        if !path.exists() {
            return Err(MemoryError::PrefNotFound(name.into()));
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    pub fn prefs_context(&self, cap: usize) -> Result<String, MemoryError> {
        let prefs = self.pref_list()?;
        if prefs.is_empty() {
            return Ok(String::new());
        }
        let mut out = String::from("Preferences:\n");
        for (name, body) in prefs {
            let chunk = format!("- {name}: {}\n", body.trim());
            if out.len() + chunk.len() > cap {
                break;
            }
            out.push_str(&chunk);
        }
        Ok(out)
    }

    pub fn retrieve_context(&self, query: &str) -> Result<String, MemoryError> {
        let q = query.to_ascii_lowercase();
        let tokens: Vec<&str> = q.split(|c: char| !c.is_alphanumeric()).filter(|t| t.len() > 2).collect();
        if tokens.is_empty() {
            return Ok(String::new());
        }
        let mut hits: Vec<String> = Vec::new();
        for sub in ["episodes", "playbooks", "tools"] {
            let dir = self.root.join("memory").join(sub);
            if !dir.exists() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let text = fs::read_to_string(&path).unwrap_or_default();
                let lower = text.to_ascii_lowercase();
                let score = tokens.iter().filter(|t| lower.contains(*t)).count();
                if score > 0 {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                    let snippet: String = text.chars().take(240).collect();
                    hits.push(format!("[{sub}/{name}] {snippet}"));
                }
            }
        }
        hits.sort();
        hits.truncate(8);
        if hits.is_empty() {
            Ok(String::new())
        } else {
            Ok(hits.join("\n"))
        }
    }

    pub fn write_episode(
        &self,
        query: &str,
        reply: &str,
        verdict: &str,
        edit: Option<&str>,
    ) -> Result<Episode, MemoryError> {
        let dir = self.root.join("memory/episodes");
        fs::create_dir_all(&dir)?;
        let id = format!("ep-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let ep = Episode {
            id: id.clone(),
            created_at: Utc::now().to_rfc3339(),
            query: query.to_string(),
            reply: reply.to_string(),
            verdict: verdict.to_string(),
            edit: edit.map(|s| s.to_string()),
        };
        let path = dir.join(format!("{id}.json"));
        fs::write(&path, serde_json::to_string_pretty(&ep)? + "\n")?;
        if matches!(verdict, "accept" | "edit") {
            let _ = self.maybe_promote_playbook(&ep);
        }
        Ok(ep)
    }

    fn maybe_promote_playbook(&self, ep: &Episode) -> Result<(), MemoryError> {
        let dir = self.root.join("memory/episodes");
        if !dir.exists() {
            return Ok(());
        }
        let tokens: Vec<String> = ep
            .query
            .to_ascii_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 3)
            .map(|s| s.to_string())
            .collect();
        if tokens.is_empty() {
            return Ok(());
        }
        let mut similar = 0u32;
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap_or_default();
            let other: Episode = match serde_json::from_str(&text) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if other.id == ep.id {
                continue;
            }
            if !matches!(other.verdict.as_str(), "accept" | "edit") {
                continue;
            }
            let lower = other.query.to_ascii_lowercase();
            let score = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if score >= 2 {
                similar += 1;
            }
        }
        if similar >= 1 {
            let pb_dir = self.root.join("memory/playbooks");
            fs::create_dir_all(&pb_dir)?;
            let key = tokens.first().cloned().unwrap_or_else(|| "general".into());
            let path = pb_dir.join(format!("{key}.md"));
            if !path.exists() {
                let body = format!(
                    "# Playbook: {key}\n\nDerived from accepted episodes.\n\nQuery pattern: {}\n\nSuccessful reply sketch:\n{}\n",
                    ep.query,
                    ep.edit.as_deref().unwrap_or(&ep.reply)
                );
                fs::write(&path, body)?;
            }
        }
        Ok(())
    }

    pub fn plot_dir(&self, plot: &str) -> Result<PathBuf, MemoryError> {
        if plot.is_empty()
            || plot.contains("..")
            || !plot.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(MemoryError::InvalidPlotName(plot.into()));
        }
        let p = self.root.join("plots").join(plot);
        if !p.is_dir() {
            return Err(MemoryError::PlotNotFound(plot.into()));
        }
        Ok(p)
    }

    /// Resolve a plot-relative path. Rejects `..`, absolute paths, and `.helix`.
    /// Empty `rel` means the plot root itself.
    pub fn plot_resolve(&self, plot: &str, rel: &str) -> Result<PathBuf, MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let rel = rel.trim().trim_start_matches('/').trim_start_matches('\\');
        if rel.is_empty() {
            return Ok(plot_dir);
        }
        for comp in Path::new(rel).components() {
            match comp {
                Component::Normal(name) => {
                    let s = name.to_string_lossy();
                    if s == ".helix" {
                        return Err(MemoryError::InvalidPlotPath(rel.into()));
                    }
                    if s.contains('\0') {
                        return Err(MemoryError::InvalidPlotPath(rel.into()));
                    }
                }
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(MemoryError::InvalidPlotPath(rel.into()));
                }
            }
        }
        let joined = plot_dir.join(rel);
        let plot_canon = plot_dir.canonicalize().unwrap_or(plot_dir.clone());
        match joined.canonicalize() {
            Ok(c) => {
                if !c.starts_with(&plot_canon) {
                    return Err(MemoryError::InvalidPlotPath(rel.into()));
                }
                Ok(c)
            }
            Err(_) => {
                if let Some(parent) = joined.parent() {
                    if parent.exists() {
                        let pc = parent.canonicalize().unwrap_or(parent.to_path_buf());
                        if !pc.starts_with(&plot_canon) {
                            return Err(MemoryError::InvalidPlotPath(rel.into()));
                        }
                    }
                }
                Ok(joined)
            }
        }
    }

    /// List entries under a plot-relative directory (non-recursive). Hides `.helix`.
    pub fn plot_list_files(&self, plot: &str, rel: &str) -> Result<Vec<PlotEntry>, MemoryError> {
        let dir = self.plot_resolve(plot, rel)?;
        if !dir.exists() {
            return Err(MemoryError::PlotPathNotFound(rel.into()));
        }
        if !dir.is_dir() {
            return Err(MemoryError::PlotPathNotDir(rel.into()));
        }
        let plot_dir = self.plot_dir(plot)?;
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == ".helix" {
                continue;
            }
            let path = entry.path();
            let rel_path = path
                .strip_prefix(&plot_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(PlotEntry {
                path: rel_path,
                is_dir: path.is_dir(),
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Read a UTF-8 text file under the plot.
    pub fn plot_read_file(&self, plot: &str, rel: &str) -> Result<String, MemoryError> {
        if rel.trim().is_empty() {
            return Err(MemoryError::InvalidPlotPath("(empty)".into()));
        }
        let path = self.plot_resolve(plot, rel)?;
        if !path.exists() {
            return Err(MemoryError::PlotPathNotFound(rel.into()));
        }
        if path.is_dir() {
            return Err(MemoryError::PlotPathIsDir(rel.into()));
        }
        Ok(fs::read_to_string(&path)?)
    }

    /// Write a UTF-8 text file under the plot (creates parent dirs).
    pub fn plot_write_file(&self, plot: &str, rel: &str, content: &str) -> Result<(), MemoryError> {
        if rel.trim().is_empty() {
            return Err(MemoryError::InvalidPlotPath("(empty)".into()));
        }
        let path = self.plot_resolve(plot, rel)?;
        if path.is_dir() {
            return Err(MemoryError::PlotPathIsDir(rel.into()));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
        Ok(())
    }

    /// Delete a file (not a directory) under the plot.
    pub fn plot_delete_file(&self, plot: &str, rel: &str) -> Result<(), MemoryError> {
        if rel.trim().is_empty() {
            return Err(MemoryError::InvalidPlotPath("(empty)".into()));
        }
        let path = self.plot_resolve(plot, rel)?;
        if !path.exists() {
            return Err(MemoryError::PlotPathNotFound(rel.into()));
        }
        if path.is_dir() {
            return Err(MemoryError::PlotPathIsDir(rel.into()));
        }
        fs::remove_file(&path)?;
        Ok(())
    }

    fn helix_dir(plot_dir: &Path) -> PathBuf {
        plot_dir.join(".helix")
    }

    fn commits_dir(plot_dir: &Path) -> PathBuf {
        Self::helix_dir(plot_dir).join("commits")
    }

    fn head_path(plot_dir: &Path) -> PathBuf {
        Self::helix_dir(plot_dir).join("HEAD")
    }

    fn tree_manifest(plot_dir: &Path) -> Result<Vec<(String, String)>, MemoryError> {
        let mut entries = Vec::new();
        fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), MemoryError> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name();
                if name == ".helix" {
                    continue;
                }
                if path.is_dir() {
                    walk(base, &path, out)?;
                } else if path.is_file() {
                    let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    let bytes = fs::read(&path)?;
                    let mut hasher = Sha256::new();
                    hasher.update(&bytes);
                    let digest = hex::encode(hasher.finalize());
                    out.push((rel, digest));
                }
            }
            Ok(())
        }
        walk(plot_dir, plot_dir, &mut entries)?;
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }

    fn manifest_id(manifest: &[(String, String)]) -> String {
        let mut hasher = Sha256::new();
        for (path, dig) in manifest {
            hasher.update(path.as_bytes());
            hasher.update(b"\0");
            hasher.update(dig.as_bytes());
            hasher.update(b"\n");
        }
        hex::encode(hasher.finalize())
    }

    pub fn plot_commit(&self, plot: &str, message: Option<&str>) -> Result<CommitMeta, MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let manifest = Self::tree_manifest(&plot_dir)?;
        let id = Self::manifest_id(&manifest);
        let commit_root = Self::commits_dir(&plot_dir).join(&id);
        let tree_dir = commit_root.join("tree");
        fs::create_dir_all(&tree_dir)?;
        for (rel, _) in &manifest {
            let src = plot_dir.join(rel);
            let dst = tree_dir.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, &dst)?;
        }
        let meta = CommitMeta {
            id: id.clone(),
            created_at: Utc::now().to_rfc3339(),
            message: message.map(|s| s.to_string()),
            file_count: manifest.len(),
        };
        fs::write(commit_root.join("meta.json"), serde_json::to_string_pretty(&meta)? + "\n")?;
        fs::write(Self::head_path(&plot_dir), format!("{id}\n"))?;
        Ok(meta)
    }

    pub fn plot_list(&self, plot: &str) -> Result<Vec<CommitMeta>, MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let dir = Self::commits_dir(&plot_dir);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let meta_path = entry.path().join("meta.json");
            if !meta_path.exists() {
                continue;
            }
            let text = fs::read_to_string(&meta_path)?;
            if let Ok(m) = serde_json::from_str::<CommitMeta>(&text) {
                out.push(m);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    pub fn plot_head(&self, plot: &str) -> Result<Option<String>, MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let path = Self::head_path(&plot_dir);
        if !path.exists() {
            return Ok(None);
        }
        let text = fs::read_to_string(&path)?;
        let id = text.trim().to_string();
        if id.is_empty() {
            Ok(None)
        } else {
            Ok(Some(id))
        }
    }

    pub fn plot_status(&self, plot: &str) -> Result<(Option<String>, String), MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let head = self.plot_head(plot)?;
        let manifest = Self::tree_manifest(&plot_dir)?;
        let working = Self::manifest_id(&manifest);
        Ok((head, working))
    }

    pub fn plot_rewind(&self, plot: &str, prefix: &str) -> Result<CommitMeta, MemoryError> {
        let plot_dir = self.plot_dir(plot)?;
        let commits = self.plot_list(plot)?;
        let matches: Vec<&CommitMeta> = commits.iter().filter(|c| c.id.starts_with(prefix)).collect();
        if matches.is_empty() {
            return Err(MemoryError::CommitNotFound(prefix.into()));
        }
        if matches.len() > 1 && !matches.iter().any(|c| c.id == prefix) {
            return Err(MemoryError::AmbiguousCommit(prefix.into()));
        }
        let meta = matches.iter().find(|c| c.id == prefix).copied().unwrap_or(matches[0]).clone();
        let tree_dir = Self::commits_dir(&plot_dir).join(&meta.id).join("tree");
        if !tree_dir.is_dir() {
            return Err(MemoryError::CommitNotFound(meta.id.clone()));
        }
        Self::clear_working_tree(&plot_dir)?;
        fn copy_tree(src: &Path, dst: &Path) -> Result<(), MemoryError> {
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name();
                let target = dst.join(&name);
                if path.is_dir() {
                    fs::create_dir_all(&target)?;
                    copy_tree(&path, &target)?;
                } else {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(&path, &target)?;
                }
            }
            Ok(())
        }
        copy_tree(&tree_dir, &plot_dir)?;
        fs::write(Self::head_path(&plot_dir), format!("{}\n", meta.id))?;
        Ok(meta)
    }

    fn clear_working_tree(plot_dir: &Path) -> Result<(), MemoryError> {
        for entry in fs::read_dir(plot_dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_name() == ".helix" {
                continue;
            }
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home() -> HelixHome {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("helix-mem-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init("hearthside").unwrap();
        home
    }

    #[test]
    fn init_layout() {
        let h = temp_home();
        assert!(h.root.join("plots/default").is_dir());
        assert!(h.root.join("memory/episodes").is_dir());
        assert_eq!(h.read_pack().unwrap(), "hearthside");
        let _ = fs::remove_dir_all(&h.root);
    }

    #[test]
    fn prefs_roundtrip() {
        let h = temp_home();
        h.pref_add("tone", "Be concise.").unwrap();
        let list = h.pref_list().unwrap();
        assert_eq!(list.len(), 1);
        let ctx = h.prefs_context(PREFS_CONTEXT_CHAR_CAP).unwrap();
        assert!(ctx.contains("tone"));
        h.pref_delete("tone").unwrap();
        assert!(h.pref_list().unwrap().is_empty());
        let _ = fs::remove_dir_all(&h.root);
    }

    #[test]
    fn episode_write() {
        let h = temp_home();
        let ep = h.write_episode("hello", "hi there", "accept", None).unwrap();
        assert!(ep.id.starts_with("ep-"));
        let path = h.root.join("memory/episodes").join(format!("{}.json", ep.id));
        assert!(path.exists());
        let _ = fs::remove_dir_all(&h.root);
    }

    #[test]
    fn plot_commit_rewind() {
        let h = temp_home();
        let plot = h.root.join("plots/default");
        fs::write(plot.join("note.txt"), "v1").unwrap();
        let c1 = h.plot_commit("default", Some("first")).unwrap();
        assert_eq!(c1.file_count, 1);
        fs::write(plot.join("note.txt"), "v2").unwrap();
        let c2 = h.plot_commit("default", Some("second")).unwrap();
        assert_ne!(c1.id, c2.id);
        let restored = h.plot_rewind("default", &c1.id[..12]).unwrap();
        assert_eq!(restored.id, c1.id);
        let body = fs::read_to_string(plot.join("note.txt")).unwrap();
        assert_eq!(body, "v1");
        let _ = fs::remove_dir_all(&h.root);
    }

    #[test]
    fn plot_files_roundtrip() {
        let h = temp_home();
        h.plot_write_file("default", "notes/hello.txt", "hello plot").unwrap();
        let body = h.plot_read_file("default", "notes/hello.txt").unwrap();
        assert_eq!(body, "hello plot");
        let entries = h.plot_list_files("default", "").unwrap();
        assert!(entries.iter().any(|e| e.path == "notes" && e.is_dir));
        let nested = h.plot_list_files("default", "notes").unwrap();
        assert!(nested.iter().any(|e| e.path == "notes/hello.txt" && !e.is_dir));
        h.plot_delete_file("default", "notes/hello.txt").unwrap();
        assert!(h.plot_read_file("default", "notes/hello.txt").is_err());
        let _ = fs::remove_dir_all(&h.root);
    }

    #[test]
    fn plot_files_reject_escape() {
        let h = temp_home();
        assert!(h.plot_resolve("default", "../outside").is_err());
        assert!(h.plot_resolve("default", ".helix/secret").is_err());
        assert!(h.plot_resolve("default", "/etc/passwd").is_err());
        let _ = fs::remove_dir_all(&h.root);
    }
}
