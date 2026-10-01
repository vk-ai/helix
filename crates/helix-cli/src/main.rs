use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use helix_atlas::{Atlas, Trust, UNTRUSTED_FUEL_CAP};
use helix_charter::Charter;
use helix_hands::{run_module, HandsConfig};
use helix_mail::Mailbox;
use helix_memory::{HelixHome, PREFS_CONTEXT_CHAR_CAP};
use helix_protocol::{
    AskRequest, AskResponse, AttenuateTokenRequest, CreateGrantRequest, DecideGrantRequest,
    GrantDecision, GrantListResponse, IssueTokenRequest, Status, VerifyTokenRequest,
    VerifyTokenResponse, DEFAULT_BIND,
};
use helix_reliquary::Reliquary;

mod browser;

#[derive(Parser)]
#[command(
    name = "helix",
    version,
    about = "Helix local-first personal agent CLI"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[arg(long, default_value = "hearthside")]
        pack: String,
    },
    Status,
    Ask {
        text: String,
        #[arg(long)]
        accept: bool,
        #[arg(long)]
        reject: bool,
        #[arg(long)]
        edit: Option<String>,
    },
    Charter {
        #[command(subcommand)]
        cmd: CharterCmd,
    },
    Pref {
        #[command(subcommand)]
        cmd: PrefCmd,
    },
    Secrets {
        #[command(subcommand)]
        cmd: SecretsCmd,
    },
    Grant {
        #[command(subcommand)]
        cmd: GrantCmd,
    },
    Token {
        #[command(subcommand)]
        cmd: TokenCmd,
    },
    Hands {
        #[command(subcommand)]
        cmd: HandsCmd,
    },
    Atlas {
        #[command(subcommand)]
        cmd: AtlasCmd,
    },
    Plot {
        #[command(subcommand)]
        cmd: PlotCmd,
    },
    Files {
        #[command(subcommand)]
        cmd: FilesCmd,
    },
    /// Mail adapter (desk+): local mailbox, Reliquary OAuth ref, Ask-gated send
    Mail {
        #[command(subcommand)]
        cmd: MailCmd,
    },
    /// Browser adapter (desk+): dedicated profile pane; pause on challenge
    Browser {
        #[command(subcommand)]
        cmd: BrowserCmd,
    },
}

#[derive(Subcommand)]
enum CharterCmd {
    Show,
    Set { pack: String },
}

#[derive(Subcommand)]
enum PrefCmd {
    Add { name: String, body: String },
    List,
    Delete { name: String },
}

#[derive(Subcommand)]
enum SecretsCmd {
    List,
    Add {
        name: String,
        #[arg(long)]
        value: Option<String>,
        #[arg(long)]
        keychain: bool,
    },
    Revoke {
        name: String,
    },
}

#[derive(Subcommand)]
enum GrantCmd {
    List,
    Request {
        action: String,
        summary: String,
        #[arg(long)]
        requester: Option<String>,
    },
    AllowOnce {
        id: String,
    },
    AllowTask {
        id: String,
    },
    Deny {
        id: String,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    Issue {
        #[arg(long)]
        rights: Option<String>,
        #[arg(long)]
        ttl: Option<u64>,
    },
    Attenuate {
        token: String,
        #[arg(long)]
        keep: String,
        #[arg(long)]
        ttl: Option<u64>,
    },
    Verify {
        token: String,
        #[arg(long)]
        require: Option<String>,
    },
    Show {
        token: String,
    },
}

#[derive(Subcommand)]
enum HandsCmd {
    Run {
        module: PathBuf,
        #[arg(long, default_value = "default")]
        plot: String,
        #[arg(long)]
        fuel: Option<u64>,
        #[arg(last = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum AtlasCmd {
    List,
    Pin { name: String, path: PathBuf },
    Unpin { name: String },
    Verify { name: String },
}

#[derive(Subcommand)]
enum PlotCmd {
    Commit {
        #[arg(long, short = 'm')]
        message: Option<String>,
        #[arg(long, default_value = "default")]
        plot: String,
    },
    List {
        #[arg(long, default_value = "default")]
        plot: String,
    },
    Rewind {
        prefix: String,
        #[arg(long, default_value = "default")]
        plot: String,
    },
    Status {
        #[arg(long, default_value = "default")]
        plot: String,
    },
}

#[derive(Subcommand)]
enum FilesCmd {
    Ls {
        #[arg(default_value = "")]
        path: String,
        #[arg(long, default_value = "default")]
        plot: String,
    },
    Read {
        path: String,
        #[arg(long, default_value = "default")]
        plot: String,
    },
    Write {
        path: String,
        #[arg(long)]
        content: Option<String>,
        #[arg(long, default_value = "default")]
        plot: String,
    },
    Delete {
        path: String,
        #[arg(long, default_value = "default")]
        plot: String,
    },
}

#[derive(Subcommand)]
enum MailCmd {
    Status,
    List {
        #[arg(long, default_value = "inbox")]
        folder: String,
        #[arg(long)]
        safe: bool,
    },
    Read {
        id: String,
        #[arg(long)]
        safe: bool,
    },
    Draft {
        to: String,
        #[arg(long, default_value = "")]
        subject: String,
        #[arg(long)]
        body: Option<String>,
        #[arg(long)]
        oauth_ref: Option<String>,
    },
    Seed {
        #[arg(long, default_value = "alice@example.com")]
        from: String,
        #[arg(long, default_value = "Hello from Helix")]
        subject: String,
        #[arg(long)]
        body: Option<String>,
    },
    Send {
        id: String,
        #[arg(long)]
        grant: String,
    },
}

#[derive(Subcommand)]
enum BrowserCmd {
    Status,
    /// Record a navigation in the dedicated profile (pauses on challenge URLs)
    Open {
        url: String,
    },
    /// Human take-over after a paused challenge (no solver)
    Resume,
    /// Print the loopback pane URL
    Pane,
}
