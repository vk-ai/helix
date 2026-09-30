//! Mail adapter for Helix (desk+ packs).
//!
//! Local-first mailbox under `~/Helix/mail/`. Read and draft are allowed when
//! the active charter has `allow_network_adapters`. Send always requires an
//! Ask grant (`mail.send`). OAuth material is referenced by Reliquary name only;
//! values are never printed or put in model context.
//!
//! OTP codes and magic-link URLs are redacted from bodies exposed to prompts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use helix_charter::Charter;
use helix_memory::HelixHome;
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MailError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("mail requires desk or workshop pack (allow_network_adapters); active={0}")]
    CharterDenied(String),
    #[error("message not found: {0}")]
    NotFound(String),
    #[error("invalid message id: {0}")]
    InvalidId(String),
    #[error("send requires an Ask grant for action mail.send")]
    NeedsGrant,
    #[error("oauth secret reference not found in Reliquary: {0}")]
    OAuthRefMissing(String),
    #[error("empty field: {0}")]
    EmptyField(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub created_at: String,
    pub folder: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafeMessage {
    pub id: String,
    pub folder: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
}

pub struct Mailbox {
    home: HelixHome,
}

impl Mailbox {
    pub fn open(home: HelixHome) -> Result<Self, MailError> {
        let m = Self { home };
        m.ensure_dirs()?;
        Ok(m)
    }

    fn mail_dir(&self) -> PathBuf {
        self.home.root.join("mail")
    }

    fn folder_dir(&self, folder: &str) -> PathBuf {
        self.mail_dir().join(folder)
    }

    fn ensure_dirs(&self) -> Result<(), MailError> {
        for f in ["inbox", "drafts", "outbox"] {
            fs::create_dir_all(self.folder_dir(f))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(self.mail_dir(), fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    pub fn require_desk_plus(pack: &str) -> Result<Charter, MailError> {
        let c = Charter::builtin(pack).map_err(|e| {
            MailError::Io(io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
        })?;
        if !c.allow_network_adapters {
            return Err(MailError::CharterDenied(c.pack));
        }
        Ok(c)
    }

    fn msg_path(&self, folder: &str, id: &str) -> Result<PathBuf, MailError> {
        validate_id(id)?;
        Ok(self.folder_dir(folder).join(format!("{id}.json")))
    }

    pub fn list(&self, folder: &str) -> Result<Vec<Message>, MailError> {
        let dir = self.folder_dir(folder);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if let Ok(m) = serde_json::from_str::<Message>(&text) {
                out.push(m);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out)
    }

    pub fn read(&self, id: &str) -> Result<Message, MailError> {
        validate_id(id)?;
        for folder in ["inbox", "drafts", "outbox"] {
            let path = self.msg_path(folder, id)?;
            if path.exists() {
                let text = fs::read_to_string(&path)?;
                return Ok(serde_json::from_str(&text)?);
            }
        }
        Err(MailError::NotFound(id.into()))
    }

    pub fn draft(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        oauth_ref: Option<&str>,
    ) -> Result<Message, MailError> {
        if to.trim().is_empty() {
            return Err(MailError::EmptyField("to".into()));
        }
        let id = format!("msg-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let msg = Message {
            id: id.clone(),
            created_at: Utc::now().to_rfc3339(),
            folder: "drafts".into(),
            from: "me@local".into(),
            to: to.trim().to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            oauth_ref: oauth_ref.map(|s| s.to_string()),
            grant_id: None,
        };
        let path = self.msg_path("drafts", &id)?;
        fs::write(&path, serde_json::to_string_pretty(&msg)? + "\n")?;
        Ok(msg)
    }

    pub fn seed_inbox(&self, from: &str, subject: &str, body: &str) -> Result<Message, MailError> {
        let id = format!("msg-{}", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let msg = Message {
            id: id.clone(),
            created_at: Utc::now().to_rfc3339(),
            folder: "inbox".into(),
            from: from.to_string(),
            to: "me@local".into(),
            subject: subject.to_string(),
            body: body.to_string(),
            oauth_ref: None,
            grant_id: None,
        };
        let path = self.msg_path("inbox", &id)?;
        fs::write(&path, serde_json::to_string_pretty(&msg)? + "\n")?;
        Ok(msg)
    }

    pub fn send_with_grant(&self, id: &str, grant_id: &str) -> Result<Message, MailError> {
        validate_id(id)?;
        let draft_path = self.msg_path("drafts", id)?;
        if !draft_path.exists() {
            return Err(MailError::NotFound(id.into()));
        }
        let text = fs::read_to_string(&draft_path)?;
        let mut msg: Message = serde_json::from_str(&text)?;
        msg.folder = "outbox".into();
        msg.grant_id = Some(grant_id.to_string());
        let out_path = self.msg_path("outbox", id)?;
        fs::write(&out_path, serde_json::to_string_pretty(&msg)? + "\n")?;
        fs::remove_file(&draft_path)?;
        let _ = self.home.append_chronicle(
            &serde_json::json!({
                "ts": Utc::now().to_rfc3339(),
                "event": "mail.send",
                "id": id,
                "grant_id": grant_id,
                "to": msg.to,
                "subject": msg.subject,
            })
            .to_string(),
        );
        Ok(msg)
    }

    pub fn safe_view(msg: &Message) -> SafeMessage {
        SafeMessage {
            id: msg.id.clone(),
            folder: msg.folder.clone(),
            from: msg.from.clone(),
            to: msg.to.clone(),
            subject: redact_text(&msg.subject),
            body: redact_text(&msg.body),
        }
    }

    pub fn status(&self) -> Result<String, MailError> {
        let inbox = self.list("inbox")?.len();
        let drafts = self.list("drafts")?.len();
        let outbox = self.list("outbox")?.len();
        Ok(format!(
            "mail_dir={} inbox={inbox} drafts={drafts} outbox={outbox}",
            self.mail_dir().display()
        ))
    }
}

fn validate_id(id: &str) -> Result<(), MailError> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(MailError::InvalidId(id.into()));
    }
    Ok(())
}

pub fn redact_text(input: &str) -> String {
    let otp = Regex::new(r"\b(\d{4,8})\b").expect("otp regex");
    let magic = Regex::new(r"(?i)https?://[^\s]+(?:magic|login|auth|verify|token|otp|reset)[^\s]*")
        .expect("magic regex");
    let step1 = magic.replace_all(input, "[REDACTED-LINK]");
    let step2 = otp.replace_all(&step1, "[REDACTED-CODE]");
    step2.into_owned()
}

pub fn ensure_mail_layout(root: &Path) -> io::Result<()> {
    for f in ["inbox", "drafts", "outbox"] {
        fs::create_dir_all(root.join("mail").join(f))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(root.join("mail"), fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_mail() -> Mailbox {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("helix-mail-test-{n}"));
        let _ = fs::remove_dir_all(&root);
        let home = HelixHome { root };
        home.init("desk").unwrap();
        Mailbox::open(home).unwrap()
    }

    #[test]
    fn desk_allowed_hearthside_denied() {
        assert!(Mailbox::require_desk_plus("desk").is_ok());
        assert!(Mailbox::require_desk_plus("workshop").is_ok());
        assert!(matches!(
            Mailbox::require_desk_plus("hearthside"),
            Err(MailError::CharterDenied(_))
        ));
    }

    #[test]
    fn draft_list_send() {
        let m = temp_mail();
        let d = m
            .draft(
                "alice@example.com",
                "Hello",
                "Body text",
                Some("mail-oauth"),
            )
            .unwrap();
        assert_eq!(d.folder, "drafts");
        assert_eq!(m.list("drafts").unwrap().len(), 1);
        let sent = m.send_with_grant(&d.id, "g-test").unwrap();
        assert_eq!(sent.folder, "outbox");
        assert_eq!(sent.grant_id.as_deref(), Some("g-test"));
        assert!(m.list("drafts").unwrap().is_empty());
        assert_eq!(m.list("outbox").unwrap().len(), 1);
        let _ = fs::remove_dir_all(&m.home.root);
    }

    #[test]
    fn redact_otp_and_magic_link() {
        let raw = "Your code is 482913. Click https://auth.example.com/magic?token=abc to login.";
        let safe = redact_text(raw);
        assert!(!safe.contains("482913"));
        assert!(safe.contains("[REDACTED-CODE]"));
        assert!(safe.contains("[REDACTED-LINK]"));
    }

    #[test]
    fn safe_view_hides_codes() {
        let msg = Message {
            id: "msg-1".into(),
            created_at: "t".into(),
            folder: "inbox".into(),
            from: "a@b.c".into(),
            to: "me".into(),
            subject: "OTP 123456".into(),
            body: "Use 998877".into(),
            oauth_ref: None,
            grant_id: None,
        };
        let s = Mailbox::safe_view(&msg);
        assert!(!s.subject.contains("123456"));
        assert!(!s.body.contains("998877"));
    }
}
