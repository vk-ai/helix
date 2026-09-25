use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use helix_atlas::{Atlas, Trust, UNTRUSTED_FUEL_CAP};
use helix_charter::Charter;
use helix_hands::{default_plot_dir, run_module, HandsConfig};
use helix_memory::{HelixHome, PREFS_CONTEXT_CHAR_CAP};
use helix_protocol::{
    AskRequest, AskResponse, AttenuateTokenRequest, CreateGrantRequest, DecideGrantRequest,
    GrantDecision, GrantListResponse, IssueTokenRequest, Status, VerifyTokenRequest,
    VerifyTokenResponse, DEFAULT_BIND,
};
use helix_reliquary::Reliquary;

#[derive(Parser)]
#[command(name = "helix", version, about = "Helix local-first personal agent CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create ~/Helix layout and default charter
    Init {
        #[arg(long, default_value = "hearthside")]
        pack: String,
    },
    /// Show home, pack, Switch, Ollama reachability
    Status,
    /// Ask the local agent (via helixd)
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
    Revoke { name: String },
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
    AllowOnce { id: String },
    AllowTask { id: String },
    Deny { id: String },
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
    Show { token: String },
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
                    let prefs = home.prefs_context(PREFS_CONTEXT_CHAR_CAP).unwrap_or_default();
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
        Commands::Token { cmd } => match cmd {
            TokenCmd::Issue { rights, ttl } => {
                let rights_vec: Vec<String> = rights
                    .map(|s| {
                        s.split(',')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                let url = format!("{}/v1/tokens", base_url());
                let res = http()
                    .post(&url)
                    .json(&IssueTokenRequest {
                        rights: rights_vec,
                        ttl_secs: ttl,
                    })
                    .send()?;
                let body = res.error_for_status()?.text()?;
                println!("{body}");
            }
            TokenCmd::Attenuate { token, keep, ttl } => {
                let token_val = load_token_json(&token)?;
                let keep_vec: Vec<String> = keep
                    .split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect();
                let url = format!("{}/v1/tokens/attenuate", base_url());
                let res = http()
                    .post(&url)
                    .json(&AttenuateTokenRequest {
                        token: token_val,
                        keep: keep_vec,
                        ttl_secs: ttl,
                    })
                    .send()?;
                let body = res.error_for_status()?.text()?;
                println!("{body}");
            }
            TokenCmd::Verify { token, require } => {
                let token_val = load_token_json(&token)?;
                let url = format!("{}/v1/tokens/verify", base_url());
                let res = http()
                    .post(&url)
                    .json(&VerifyTokenRequest {
                        token: token_val,
                        require,
                    })
                    .send()?;
                let body: VerifyTokenResponse = res.error_for_status()?.json()?;
                if body.valid {
                    println!("valid=true");
                    if let Some(r) = body.rights {
                        println!("rights={}", r.join(","));
                    }
                } else {
                    println!("valid=false");
                    if let Some(e) = body.error {
                        println!("error={e}");
                    }
                }
            }
            TokenCmd::Show { token } => {
                let token_val = load_token_json(&token)?;
                println!("{}", serde_json::to_string_pretty(&token_val)?);
            }
        },
        Commands::Hands { cmd } => match cmd {
            HandsCmd::Run {
                module,
                plot,
                fuel,
                args,
            } => {
                let home = HelixHome::resolve()?;
                let plot_dir = if plot == "default" {
                    default_plot_dir(&home.root)
                } else {
                    home.plot_dir(&plot)?
                };
                let wasm = std::fs::read(&module)?;
                let atlas = Atlas::open(home.clone());
                let trust = atlas.classify(&wasm, module.to_str())?;
                let mut effective_fuel = fuel;
                match &trust {
                    Trust::Trusted { name, sha256 } => {
                        eprintln!("atlas: trusted pin '{name}' sha256={sha256}");
                    }
                    Trust::Untrusted {
                        reason,
                        sha256,
                        fuel_cap,
                    } => {
                        eprintln!("atlas: UNTRUSTED ({reason}) sha256={sha256}");
                        if effective_fuel.is_none() {
                            effective_fuel = Some(*fuel_cap);
                            eprintln!("atlas: applying default fuel cap {UNTRUSTED_FUEL_CAP}");
                        }
                    }
                }
                let mut argv = vec![module
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("module")
                    .to_string()];
                argv.extend(args);
                let mut cfg = HandsConfig::for_plot(&plot_dir)
                    .with_args(argv)
                    .with_inherit_stdio(true);
                if let Some(f) = effective_fuel {
                    cfg = cfg.with_fuel(f);
                }
                let result = run_module(&wasm, &cfg)?;
                if result.exit_code != 0 {
                    anyhow::bail!("module exited with {}", result.exit_code);
                }
            }
        },
        Commands::Atlas { cmd } => {
            let home = HelixHome::resolve()?;
            let atlas = Atlas::open(home);
            match cmd {
                AtlasCmd::List => {
                    for p in atlas.list()? {
                        println!(
                            "{}  sha256={}  trusted={}  path={}",
                            p.name, p.sha256, p.trusted, p.path
                        );
                    }
                }
                AtlasCmd::Pin { name, path } => {
                    let pin = atlas.pin(&name, &path)?;
                    println!("pinned {} sha256={}", pin.name, pin.sha256);
                }
                AtlasCmd::Unpin { name } => {
                    atlas.unpin(&name)?;
                    println!("unpinned {name}");
                }
                AtlasCmd::Verify { name } => {
                    let pin = atlas.verify_name(&name)?;
                    println!("ok {} sha256={}", pin.name, pin.sha256);
                }
            }
        }
        Commands::Plot { cmd } => {
            let home = HelixHome::resolve()?;
            match cmd {
                PlotCmd::Commit { message, plot } => {
                    let meta = home.plot_commit(&plot, message.as_deref())?;
                    println!("{}  files={}  {}", meta.id, meta.file_count, meta.created_at);
                    if let Some(m) = meta.message {
                        println!("message={m}");
                    }
                }
                PlotCmd::List { plot } => {
                    for m in home.plot_list(&plot)? {
                        let msg = m.message.unwrap_or_default();
                        println!("{}  files={}  {}  {msg}", m.id, m.file_count, m.created_at);
                    }
                }
                PlotCmd::Rewind { prefix, plot } => {
                    let meta = home.plot_rewind(&plot, &prefix)?;
                    println!("restored {}", meta.id);
                }
                PlotCmd::Status { plot } => {
                    let (head, working) = home.plot_status(&plot)?;
                    match head {
                        Some(h) => println!("HEAD={h}"),
                        None => println!("HEAD=(none)"),
                    }
                    println!("working={working}");
                    if head.as_deref() == Some(working.as_str()) {
                        println!("clean=true");
                    } else {
                        println!("clean=false");
                    }
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

fn load_token_json(spec: &str) -> anyhow::Result<serde_json::Value> {
    let text = if let Some(path) = spec.strip_prefix('@') {
        std::fs::read_to_string(path)?
    } else {
        spec.to_string()
    };
    Ok(serde_json::from_str(&text)?)
}
