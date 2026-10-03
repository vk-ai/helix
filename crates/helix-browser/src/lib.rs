//! Browser adapter for Helix (desk+ packs).
//!
//! Dedicated local profile under `~/Helix/browser/profile/`.
//! Navigation is recorded locally; helixd never embeds a CAPTCHA solver.
//! When a URL looks like a challenge page the session pauses until a human resumes it.

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::Utc;
use helix_charter::Charter;
use helix_memory::HelixHome;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("browser requires desk or workshop pack (allow_network_adapters); active={0}")]
    CharterDenied(String),
    #[error("session is not paused on a challenge")]
    NotPaused,
    #[error("empty url")]
    EmptyUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Ready,
    PausedChallenge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Challenge {
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub url: String,
    pub state: SessionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<Challenge>,
    pub updated_at: String,
    #[serde(default)]
    pub history: Vec<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            url: "about:blank".into(),
            state: SessionState::Idle,
            challenge: None,
            updated_at: Utc::now().to_rfc3339(),
            history: vec![],
        }
    }
}

pub struct Browser {
    home: HelixHome,
}

impl Browser {
    pub fn open(home: HelixHome) -> Result<Self, BrowserError> {
        let b = Self { home };
        b.ensure_dirs()?;
        Ok(b)
    }

    pub fn require_desk_plus(pack: &str) -> Result<Charter, BrowserError> {
        let c = Charter::builtin(pack).map_err(|e| {
            BrowserError::Io(io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
        })?;
        if !c.allow_network_adapters {
            return Err(BrowserError::CharterDenied(c.pack));
        }
        Ok(c)
    }

    fn root(&self) -> PathBuf {
        self.home.root.join("browser")
    }

    pub fn profile_dir(&self) -> PathBuf {
        self.root().join("profile")
    }

    fn session_path(&self) -> PathBuf {
        self.root().join("session.json")
    }

    fn ensure_dirs(&self) -> Result<(), BrowserError> {
        fs::create_dir_all(self.profile_dir())?;
        let marker = self.profile_dir().join("README.txt");
        if !marker.exists() {
            fs::write(
                marker,
                "Helix dedicated browser profile. Cookies and site data stay here.\n\
                 Challenges pause for a human. There is no solver in core.\n",
            )?;
        }
        if !self.session_path().exists() {
            self.write_session(&Session::default())?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.root(), fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    pub fn session(&self) -> Result<Session, BrowserError> {
        let text = fs::read_to_string(self.session_path())?;
        Ok(serde_json::from_str(&text)?)
    }

    fn write_session(&self, s: &Session) -> Result<(), BrowserError> {
        fs::write(self.session_path(), serde_json::to_string_pretty(s)? + "\n")?;
        Ok(())
    }

    pub fn navigate(&self, url: &str) -> Result<Session, BrowserError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(BrowserError::EmptyUrl);
        }
        let mut s = self.session().unwrap_or_default();
        s.url = url.to_string();
        s.updated_at = Utc::now().to_rfc3339();
        if s.history.last().map(|h| h.as_str()) != Some(url) {
            s.history.push(url.to_string());
            if s.history.len() > 40 {
                let drop_n = s.history.len() - 40;
                s.history.drain(0..drop_n);
            }
        }
        if let Some(ch) = detect_challenge(url) {
            s.state = SessionState::PausedChallenge;
            s.challenge = Some(ch);
        } else {
            s.state = SessionState::Ready;
            s.challenge = None;
        }
        self.write_session(&s)?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": Utc::now().to_rfc3339(),
                "event": "browser.navigate",
                "url": url,
                "state": format!("{:?}", s.state),
                "paused": s.state == SessionState::PausedChallenge,
            })
            .to_string(),
        );
        Ok(s)
    }

    pub fn resume(&self) -> Result<Session, BrowserError> {
        let mut s = self.session()?;
        if s.state != SessionState::PausedChallenge {
            return Err(BrowserError::NotPaused);
        }
        s.state = SessionState::Ready;
        s.challenge = None;
        s.updated_at = Utc::now().to_rfc3339();
        self.write_session(&s)?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": Utc::now().to_rfc3339(),
                "event": "browser.resume",
                "url": s.url,
                "by": "human",
            })
            .to_string(),
        );
        Ok(s)
    }

    pub fn status_line(&self) -> Result<String, BrowserError> {
        let s = self.session()?;
        Ok(format!(
            "profile={} state={:?} url={} challenge={}",
            self.profile_dir().display(),
            s.state,
            s.url,
            s.challenge.as_ref().map(|c| c.kind.as_str()).unwrap_or("-")
        ))
    }
}

pub fn detect_challenge(url: &str) -> Option<Challenge> {
    let lower = url.to_ascii_lowercase();
    let needles = [
        ("recaptcha", "recaptcha"),
        ("hcaptcha", "hcaptcha"),
        ("turnstile", "cloudflare-turnstile"),
        ("captcha", "captcha"),
        ("challenge", "challenge"),
        ("cf-challenge", "cloudflare"),
    ];
    for (needle, kind) in needles {
        if lower.contains(needle) {
            return Some(Challenge {
                kind: kind.into(),
                reason: format!("URL matched '{needle}'; paused for human"),
            });
        }
    }
    None
}

pub const PANE_PATH: &str = "/browser";
pub const PANE_API_PATH: &str = "/v1/browser";

pub fn pane_html() -> &'static str {
    PANE
}

const PANE: &str = r#"<!DOCTYPE html>
<html lang=\"en\"><head><meta charset=\"utf-8\"/><title>Helix browser profile</title>
<style>body{font-family:sans-serif;background:#111;color:#eee;margin:1rem}section{border:1px solid #333;padding:1rem;margin:1rem 0}button{margin:.25rem}.paused{color:#f6c}.empty{color:#666}</style>
</head><body>
<h1>Helix browser profile</h1>
<p id=\"hdr\">loopback pane — dedicated profile, no CAPTCHA solver</p>
<section><h2>Session</h2><div id=\"sess\"></div>
<button onclick=\"resume()\">human solved — resume</button>
</section>
<section><h2>History</h2><pre id=\"hist\"></pre></section>
<script>
async function resume() {
  await fetch('/v1/browser/resume', {method:'POST'});
  load();
}
async function load() {
  const res = await fetch('/v1/browser');
  if (!res.ok) { document.getElementById('hdr').textContent = 'browser api failed'; return; }
  const d = await res.json();
  document.getElementById('hdr').textContent = 'profile=' + d.profile + ' pack=' + d.pack;
  const paused = d.state === 'paused_challenge';
  document.getElementById('sess').innerHTML =
    '<div>url=' + (d.url||'') + '</div>' +
    '<div class=\"'+(paused?'paused':'')+'\">state=' + d.state +
    (d.challenge ? ' kind='+d.challenge.kind+' — '+d.challenge.reason : '') + '</div>' +
    '<p class=empty>Challenges pause. A human solves them outside the model.</p>';
  document.getElementById('hist').textContent = (d.history||[]).join('\n') || '(empty)';
}
load(); setInterval(load, 3000);
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_browser() -> Browser {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-browser-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init("desk").unwrap();
        Browser::open(home).unwrap()
    }

    #[test]
    fn desk_allowed_hearthside_denied() {
        assert!(Browser::require_desk_plus("desk").is_ok());
        assert!(Browser::require_desk_plus("workshop").is_ok());
        assert!(matches!(
            Browser::require_desk_plus("hearthside"),
            Err(BrowserError::CharterDenied(_))
        ));
    }

    #[test]
    fn challenge_url_pauses_and_resume_is_human() {
        let b = temp_browser();
        let s = b.navigate("https://example.com/login?captcha=1").unwrap();
        assert_eq!(s.state, SessionState::PausedChallenge);
        assert!(s.challenge.is_some());
        let s = b.resume().unwrap();
        assert_eq!(s.state, SessionState::Ready);
        assert!(s.challenge.is_none());
        let _ = fs::remove_dir_all(&b.home.root);
    }

    #[test]
    fn ordinary_url_is_ready() {
        let b = temp_browser();
        let s = b.navigate("https://example.com/notes").unwrap();
        assert_eq!(s.state, SessionState::Ready);
        let _ = fs::remove_dir_all(&b.home.root);
    }

    #[test]
    fn pane_has_no_solver() {
        let html = pane_html();
        assert!(html.contains("no CAPTCHA solver"));
        assert!(html.contains("human solved"));
        assert!(!html
            .to_ascii_lowercase()
            .contains("solve captcha automatically"));
    }
}
