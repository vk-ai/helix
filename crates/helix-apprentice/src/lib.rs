//! Workshop-only Apprentice.
//!
//! Propose, execute, and select notes for a fixed sandbox of tools. Results
//! land in `memory/tools/` as readable notes. Never on the default path:
//! hearthside and desk are refused. Reliquary is never opened.

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::Utc;
use helix_charter::Charter;
use helix_memory::HelixHome;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const INPUT_CAP: usize = 4_096;

#[derive(Debug, Error)]
pub enum ApprenticeError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("apprentice is workshop-only; active pack is {0}")]
    CharterDenied(String),
    #[error("unknown sandbox tool: {0}")]
    UnknownTool(String),
    #[error("empty task")]
    EmptyTask,
    #[error("input exceeds {INPUT_CAP} bytes")]
    InputTooLarge,
}

#[derive(Debug, Clone, Copy)]
pub struct SandboxTool {
    pub name: &'static str,
    pub summary: &'static str,
    pub keywords: &'static [&'static str],
}

/// Fixed sandbox. No shell, no network, no secret unwrap.
pub fn sandbox_tools() -> &'static [SandboxTool] {
    &[
        SandboxTool {
            name: "echo",
            summary: "Return the input unchanged. Use to check a note was received.",
            keywords: &["echo", "repeat", "copy", "passthrough"],
        },
        SandboxTool {
            name: "count_words",
            summary: "Count whitespace-separated words in the input.",
            keywords: &["count", "words", "length", "tally"],
        },
        SandboxTool {
            name: "upper",
            summary: "Return the input in ASCII uppercase.",
            keywords: &["upper", "uppercase", "caps", "shout"],
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proposal {
    pub tool: String,
    pub note: String,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Trial {
    pub tool: String,
    pub input: String,
    pub output: String,
    pub ok: bool,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Session {
    #[serde(default)]
    task: String,
    #[serde(default)]
    proposals: Vec<Proposal>,
    #[serde(default)]
    last_trial: Option<Trial>,
}

pub struct Apprentice {
    home: HelixHome,
}

impl Apprentice {
    pub fn open(home: HelixHome) -> Result<Self, ApprenticeError> {
        let a = Self { home };
        fs::create_dir_all(a.dir())?;
        Ok(a)
    }

    /// Workshop pack only. Desk and hearthside stay off the Apprentice path.
    pub fn require_workshop(pack: &str) -> Result<Charter, ApprenticeError> {
        let c = Charter::builtin(pack).map_err(|e| {
            ApprenticeError::Io(io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
        })?;
        if c.pack != "workshop" {
            return Err(ApprenticeError::CharterDenied(c.pack));
        }
        Ok(c)
    }

    fn dir(&self) -> PathBuf {
        self.home.root.join("workshop").join("apprentice")
    }

    fn session_path(&self) -> PathBuf {
        self.dir().join("session.json")
    }

    fn load_session(&self) -> Session {
        fs::read_to_string(self.session_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_session(&self, s: &Session) -> Result<(), ApprenticeError> {
        fs::create_dir_all(self.dir())?;
        fs::write(self.session_path(), serde_json::to_string_pretty(s)? + "\n")?;
        Ok(())
    }

    pub fn status_line(&self) -> Result<String, ApprenticeError> {
        let s = self.load_session();
        Ok(format!(
            "pack=workshop tools={} proposals={} last_trial={}",
            sandbox_tools().len(),
            s.proposals.len(),
            s.last_trial
                .as_ref()
                .map(|t| t.tool.as_str())
                .unwrap_or("-")
        ))
    }

    /// Rank sandbox tools against a task. Does not call a model or touch secrets.
    pub fn propose(&self, task: &str) -> Result<Vec<Proposal>, ApprenticeError> {
        let task = task.trim();
        if task.is_empty() {
            return Err(ApprenticeError::EmptyTask);
        }
        let lower = task.to_ascii_lowercase();
        let mut proposals: Vec<Proposal> = sandbox_tools()
            .iter()
            .map(|tool| {
                let mut score = 0u32;
                for kw in tool.keywords {
                    if lower.contains(kw) {
                        score += 2;
                    }
                }
                for token in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
                    if token.len() > 2 && tool.summary.to_ascii_lowercase().contains(token) {
                        score += 1;
                    }
                }
                Proposal {
                    tool: tool.name.into(),
                    note: format!(
                        "Sandbox tool `{name}`: {summary} Task: {task}",
                        name = tool.name,
                        summary = tool.summary,
                        task = task
                    ),
                    score,
                }
            })
            .collect();
        proposals.sort_by(|a, b| b.score.cmp(&a.score).then(a.tool.cmp(&b.tool)));
        let mut session = self.load_session();
        session.task = task.to_string();
        session.proposals = proposals.clone();
        self.save_session(&session)?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": Utc::now().to_rfc3339(),
                "event": "apprentice.propose",
                "task": task,
                "tools": proposals.iter().map(|p| &p.tool).collect::<Vec<_>>(),
            })
            .to_string(),
        );
        Ok(proposals)
    }

    /// Run one sandbox tool. No host shell, no network, no Reliquary unwrap.
    pub fn execute(&self, tool: &str, input: &str) -> Result<Trial, ApprenticeError> {
        if input.len() > INPUT_CAP {
            return Err(ApprenticeError::InputTooLarge);
        }
        let output = run_sandbox(tool, input)?;
        let trial = Trial {
            tool: tool.to_string(),
            input: input.to_string(),
            output,
            ok: true,
            at: Utc::now().to_rfc3339(),
        };
        let mut session = self.load_session();
        session.last_trial = Some(trial.clone());
        self.save_session(&session)?;
        let path = self.dir().join(format!("trial-{tool}.json"));
        fs::write(&path, serde_json::to_string_pretty(&trial)? + "\n")?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": trial.at,
                "event": "apprentice.execute",
                "tool": tool,
                "ok": true,
            })
            .to_string(),
        );
        Ok(trial)
    }

    /// Write the selected sandbox note into `memory/tools/`. Does not read Reliquary.
    pub fn select(&self, tool: &str) -> Result<PathBuf, ApprenticeError> {
        if sandbox_tools().iter().all(|t| t.name != tool) {
            return Err(ApprenticeError::UnknownTool(tool.into()));
        }
        let session = self.load_session();
        let note = session
            .proposals
            .iter()
            .find(|p| p.tool == tool)
            .map(|p| p.note.clone())
            .unwrap_or_else(|| {
                sandbox_tools()
                    .iter()
                    .find(|t| t.name == tool)
                    .map(|t| {
                        format!(
                            "Sandbox tool `{name}`: {summary}",
                            name = t.name,
                            summary = t.summary
                        )
                    })
                    .unwrap_or_default()
            });
        let trial_line = session
            .last_trial
            .as_ref()
            .filter(|t| t.tool == tool)
            .map(|t| format!("Last trial output: {}\n", t.output))
            .unwrap_or_default();
        let body = format!(
            "# {tool}\n\nsource: apprentice\ncharter: workshop\nreliquary: never\n\n{note}\n\n{trial_line}"
        );
        let dir = self.home.root.join("memory").join("tools");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{tool}.md"));
        fs::write(&path, body)?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": Utc::now().to_rfc3339(),
                "event": "apprentice.select",
                "tool": tool,
                "note": path.display().to_string(),
            })
            .to_string(),
        );
        Ok(path)
    }
}

fn run_sandbox(tool: &str, input: &str) -> Result<String, ApprenticeError> {
    match tool {
        "echo" => Ok(input.to_string()),
        "count_words" => {
            let n = input.split_whitespace().count();
            Ok(n.to_string())
        }
        "upper" => Ok(input.to_ascii_uppercase()),
        other => Err(ApprenticeError::UnknownTool(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home(pack: &str) -> HelixHome {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-apprentice-test-{pack}-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init(pack).unwrap();
        home.write_pack(pack).unwrap();
        home
    }

    #[test]
    fn only_workshop_is_allowed() {
        assert!(Apprentice::require_workshop("workshop").is_ok());
        assert!(matches!(
            Apprentice::require_workshop("desk"),
            Err(ApprenticeError::CharterDenied(_))
        ));
        assert!(matches!(
            Apprentice::require_workshop("hearthside"),
            Err(ApprenticeError::CharterDenied(_))
        ));
    }

    #[test]
    fn propose_execute_select_writes_tool_note_not_reliquary() {
        let home = temp_home("workshop");
        let a = Apprentice::open(home.clone()).unwrap();
        let props = a.propose("count words in a note").unwrap();
        assert_eq!(props[0].tool, "count_words");
        let trial = a.execute("count_words", "one two three").unwrap();
        assert_eq!(trial.output, "3");
        let path = a.select("count_words").unwrap();
        let note = fs::read_to_string(&path).unwrap();
        assert!(note.contains("reliquary: never"));
        assert!(note.contains("count_words"));
        assert!(!home.root.join("reliquary/catalog.json").exists());
        assert!(!home.root.join("reliquary/sealed.json").exists());
        let _ = fs::remove_dir_all(&home.root);
    }

    #[test]
    fn unknown_tool_and_shell_are_rejected() {
        let home = temp_home("workshop");
        let a = Apprentice::open(home).unwrap();
        assert!(matches!(
            a.execute("sh", "rm -rf /"),
            Err(ApprenticeError::UnknownTool(_))
        ));
        assert!(matches!(
            a.execute("cat", "/etc/passwd"),
            Err(ApprenticeError::UnknownTool(_))
        ));
    }
}
