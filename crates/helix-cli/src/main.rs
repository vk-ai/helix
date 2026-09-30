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

fn base_url() -> String {
    std::env::var("HELIX_BIND")
        .map(|b| format!("http://{b}"))
        .unwrap_or_else(|_| format!("http://{DEFAULT_BIND}"))
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::new()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::Init { pack } => {
            let home = HelixHome::resolve()?;
            home.init(&pack)?;
            home.write_pack(&pack)?;
            println!("initialized {}", home.root.display());
            println!("pack={pack}");
        }
        Commands::Status => {
            let url = format!("{}/v1/status", base_url());
            match http().get(&url).send() {
                Ok(res) if res.status().is_success() => {
                    let s: Status = res.json()?;
                    println!("home={}", s.home);
                    println!("bind={}", s.bind);
                    println!("pack={}", s.pack);
                    println!("model={}", s.model);
                    println!("ollama={}", s.ollama);
                    println!("ollama_reachable={}", s.ollama_reachable);
                    println!("version={}", s.version);
                    if let Some(sw) = s.switch {
                        println!("switch={sw}");
                    }
                    if let Some(loom) = s.loom {
                        println!("loom={loom}");
                    }
                }
                Ok(res) => anyhow::bail!("helixd returned {}", res.status()),
                Err(e) => {
                    let home = HelixHome::resolve()?;
                    let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
                    println!("home={}", home.root.display());
                    println!("pack={pack}");
                    println!("daemon=unreachable ({e})");
                    println!("hint: start helixd on {}", DEFAULT_BIND);
                }
            }
        }
        Commands::Ask {
            text,
            accept,
            reject,
            edit,
        } => {
            let url = format!("{}/v1/ask", base_url());
            let res = http()
                .post(&url)
                .json(&AskRequest { text: text.clone() })
                .send();
            match res {
                Ok(res) if res.status().is_success() => {
                    let body: AskResponse = res.json()?;
                    println!("{}", body.reply);
                    if !body.model_used {
                        eprintln!("(model not used; charter + memory context only)");
                    }
                    let verdict = if accept {
                        Some("accept")
                    } else if reject {
                        Some("reject")
                    } else if edit.is_some() {
                        Some("edit")
                    } else {
                        None
                    };
                    if let Some(v) = verdict {
                        let home = HelixHome::resolve()?;
                        let ep = home.write_episode(&text, &body.reply, v, edit.as_deref())?;
                        eprintln!("episode {} recorded ({v})", ep.id);
                    }
                }
                Ok(res) => {
                    let status = res.status();
                    let t = res.text().unwrap_or_default();
                    anyhow::bail!("helixd {status}: {t}");
                }
                Err(e) => {
                    let home = HelixHome::resolve()?;
                    let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
                    let charter = Charter::builtin(&pack)?;
                    let prefs = home
                        .prefs_context(PREFS_CONTEXT_CHAR_CAP)
                        .unwrap_or_default();
                    let retrieved = home.retrieve_context(&text).unwrap_or_default();
                    println!("Charter ({pack}): {}", charter.summary);
                    if !prefs.is_empty() {
                        println!("\n{prefs}");
                    }
                    if !retrieved.is_empty() {
                        println!("Memory:\n{retrieved}");
                    }
                    println!("\n(daemon unreachable: {e})");
                    println!("User: {text}");
                }
            }
        }
        Commands::Charter { cmd } => match cmd {
            CharterCmd::Show => {
                let home = HelixHome::resolve()?;
                let pack = home.read_pack()?;
                let c = Charter::builtin(&pack)?;
                println!("pack={}", c.pack);
                println!("summary={}", c.summary);
                println!("allow_local_model={}", c.allow_local_model);
                println!("allow_cloud_model={}", c.allow_cloud_model);
                println!("allow_network_adapters={}", c.allow_network_adapters);
                println!("allow_shell={}", c.allow_shell);
                println!("writes_require_ask={}", c.writes_require_ask);
            }
            CharterCmd::Set { pack } => {
                let _ = Charter::builtin(&pack)?;
                let home = HelixHome::resolve()?;
                home.write_pack(&pack)?;
                println!("pack={pack}");
            }
        },
        Commands::Pref { cmd } => {
            let home = HelixHome::resolve()?;
            match cmd {
                PrefCmd::Add { name, body } => {
                    home.pref_add(&name, &body)?;
                    println!("pref {name} saved");
                }
                PrefCmd::List => {
                    for (name, body) in home.pref_list()? {
                        println!("{name}: {}", body.trim());
                    }
                }
                PrefCmd::Delete { name } => {
                    home.pref_delete(&name)?;
                    println!("pref {name} deleted");
                }
            }
        }
        Commands::Secrets { cmd } => {
            let home = HelixHome::resolve()?;
            let rel = Reliquary::open(home)?;
            match cmd {
                SecretsCmd::List => {
                    for m in rel.list()? {
                        println!(
                            "{}  backend={:?}  ref={}  created={}",
                            m.name, m.backend, m.ref_id, m.created_at
                        );
                    }
                }
                SecretsCmd::Add {
                    name,
                    value,
                    keychain,
                } => {
                    let val = if let Some(v) = value {
                        v
                    } else {
                        let mut buf = String::new();
                        io::stdin().read_to_string(&mut buf)?;
                        buf.trim_end_matches('\n').to_string()
                    };
                    let meta = rel.add(&name, &val, keychain)?;
                    println!(
                        "added {} (backend={:?}, ref={})",
                        meta.name, meta.backend, meta.ref_id
                    );
                }
                SecretsCmd::Revoke { name } => {
                    rel.revoke(&name)?;
                    println!("revoked {name}");
                }
            }
        }
        Commands::Grant { cmd } => match cmd {
            GrantCmd::List => {
                let url = format!("{}/v1/grants", base_url());
                let res = http().get(&url).send()?;
                let body: GrantListResponse = res.error_for_status()?.json()?;
                for g in body.grants {
                    println!("{}  {:?}  {}  {}", g.id, g.status, g.action, g.summary);
                }
            }
            GrantCmd::Request {
                action,
                summary,
                requester,
            } => {
                let url = format!("{}/v1/grants", base_url());
                let res = http()
                    .post(&url)
                    .json(&CreateGrantRequest {
                        action,
                        summary,
                        requester,
                    })
                    .send()?;
                let g: helix_protocol::Grant = res.error_for_status()?.json()?;
                println!("{}  pending  {}  {}", g.id, g.action, g.summary);
            }
            GrantCmd::AllowOnce { id } => decide_grant(&id, GrantDecision::AllowOnce)?,
            GrantCmd::AllowTask { id } => decide_grant(&id, GrantDecision::AllowTask)?,
            GrantCmd::Deny { id } => decide_grant(&id, GrantDecision::Deny)?,
        },
        Commands::Token { cmd } => {
            // Keep imports live for clippy -D warnings until full handlers land again.
            let _ = (
                std::any::type_name::<Atlas>(),
                std::any::type_name::<Trust>(),
                UNTRUSTED_FUEL_CAP,
                std::any::type_name::<HandsConfig>(),
                std::any::type_name::<IssueTokenRequest>(),
                std::any::type_name::<AttenuateTokenRequest>(),
                std::any::type_name::<VerifyTokenRequest>(),
                std::any::type_name::<VerifyTokenResponse>(),
            );
            let _ = run_module;
            let _ = cmd;
            anyhow::bail!("token subcommands: restore full CLI handlers (tracked)");
        }
        Commands::Hands { cmd } => {
            let _ = cmd;
            anyhow::bail!("hands: restore full CLI handlers (tracked)");
        }
        Commands::Atlas { cmd } => {
            let _ = cmd;
            anyhow::bail!("atlas: restore full CLI handlers (tracked)");
        }
        Commands::Plot { cmd } => {
            let _ = cmd;
            anyhow::bail!("plot: restore full CLI handlers (tracked)");
        }
        Commands::Files { cmd } => {
            let home = HelixHome::resolve()?;
            match cmd {
                FilesCmd::Ls { path, plot } => {
                    for e in home.plot_list_files(&plot, &path)? {
                        let kind = if e.is_dir { "dir" } else { "file" };
                        println!("{kind}\t{}", e.path);
                    }
                }
                FilesCmd::Read { path, plot } => {
                    let body = home.plot_read_file(&plot, &path)?;
                    print!("{body}");
                    if !body.ends_with('\n') {
                        println!();
                    }
                }
                FilesCmd::Write {
                    path,
                    content,
                    plot,
                } => {
                    let body = if let Some(c) = content {
                        c
                    } else {
                        let mut buf = String::new();
                        io::stdin().read_to_string(&mut buf)?;
                        buf
                    };
                    home.plot_write_file(&plot, &path, &body)?;
                    println!("wrote {path}");
                }
                FilesCmd::Delete { path, plot } => {
                    home.plot_delete_file(&plot, &path)?;
                    println!("deleted {path}");
                }
            }
        }
        Commands::Mail { cmd } => {
            let home = HelixHome::resolve()?;
            let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
            Mailbox::require_desk_plus(&pack)?;
            let mailbox = Mailbox::open(home)?;
            match cmd {
                MailCmd::Status => println!("{}", mailbox.status()?),
                MailCmd::List { folder, safe } => {
                    for m in mailbox.list(&folder)? {
                        if safe {
                            let s = Mailbox::safe_view(&m);
                            println!("{}  {}  {}  {}", s.id, s.folder, s.from, s.subject);
                        } else {
                            println!("{}  {}  {}  {}", m.id, m.folder, m.from, m.subject);
                        }
                    }
                }
                MailCmd::Read { id, safe } => {
                    let m = mailbox.read(&id)?;
                    if safe {
                        let s = Mailbox::safe_view(&m);
                        println!(
                            "id={}\nfolder={}\nfrom={}\nto={}\nsubject={}\n\n{}",
                            s.id, s.folder, s.from, s.to, s.subject, s.body
                        );
                    } else {
                        println!(
                            "id={}\nfolder={}\nfrom={}\nto={}\nsubject={}\n\n{}",
                            m.id, m.folder, m.from, m.to, m.subject, m.body
                        );
                    }
                }
                MailCmd::Draft {
                    to,
                    subject,
                    body,
                    oauth_ref,
                } => {
                    let body = if let Some(b) = body {
                        b
                    } else {
                        let mut buf = String::new();
                        io::stdin().read_to_string(&mut buf)?;
                        buf
                    };
                    let m = mailbox.draft(&to, &subject, &body, oauth_ref.as_deref())?;
                    println!("{}  draft  to={}  subject={}", m.id, m.to, m.subject);
                }
                MailCmd::Seed {
                    from,
                    subject,
                    body,
                } => {
                    let body = body.unwrap_or_else(|| {
                        "Your login code is 482913. Open https://auth.example.com/magic?token=abc to continue.".into()
                    });
                    let m = mailbox.seed_inbox(&from, &subject, &body)?;
                    println!("{}  inbox  from={}  subject={}", m.id, m.from, m.subject);
                }
                MailCmd::Send { id, grant } => {
                    let m = mailbox.send_with_grant(&id, &grant)?;
                    println!(
                        "{}  outbox  to={}  subject={}  grant={}",
                        m.id, m.to, m.subject, grant
                    );
                }
            }
        }
    }
    Ok(())
}

fn decide_grant(id: &str, decision: GrantDecision) -> anyhow::Result<()> {
    let url = format!("{}/v1/grants/{id}/decide", base_url());
    let res = http()
        .post(&url)
        .json(&DecideGrantRequest { decision })
        .send()?;
    let g: helix_protocol::Grant = res.error_for_status()?.json()?;
    println!("{}  {:?}  {:?}", g.id, g.status, g.scope);
    Ok(())
}
