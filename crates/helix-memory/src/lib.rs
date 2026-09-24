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
