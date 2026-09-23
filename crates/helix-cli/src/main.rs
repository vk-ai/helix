use clap::{Parser, Subcommand};
use helix_atlas::{Atlas, Trust};
use helix_charter::Charter;
use helix_memory::{new_episode, HelixHome, Verdict};
use helix_protocol::{
    AskRequest, AskResponse, AttenuateTokenRequest, CreateGrantRequest, DecideGrantRequest, Grant,
    GrantDecision, GrantListResponse, IssueTokenRequest, Status, VerifyTokenRequest,
    VerifyTokenResponse, DEFAULT_BIND, DEFAULT_PACK,
};
use helix_reliquary::Reliquary;
use serde_json::Value;

#[derive(Parser)]
#[command(name = "helix", version, about = "Helix local personal agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create ~/Helix (or $HELIX_HOME) and the default charter
    Init {
        #[arg(long, default_value = DEFAULT_PACK)]
        pack: String,
    },
    /// Print daemon status
    Status,
    /// Show or switch the active charter pack
    Charter {
        #[command(subcommand)]
        action: CharterCmd,
    },
    /// Manage short preference rules (memory/prefs/)
    Pref {
        #[command(subcommand)]
        action: PrefCmd,
    },
    /// Manage Reliquary secret references (names only; values never printed)
    Secrets {
        #[command(subcommand)]
        action: SecretsCmd,
    },
    /// Approve or deny pending capability grants (Ask protocol)
    Grant {
        #[command(subcommand)]
        action: GrantCmd,
    },
    /// Shrink-only capability tokens issued by helixd
    Token {
        #[command(subcommand)]
        action: TokenCmd,
    },
    /// Run a Wasm module under Hands (plot-scoped WASI, deny-by-default)
    Hands {
        #[command(subcommand)]
        action: HandsCmd,
    },
    /// Digest-pinned tools (atlas/pins.json); untrusted tools get a narrower charter
    Atlas {
        #[command(subcommand)]
        action: AtlasCmd,
    },
    /// Send a message through the local daemon
    Ask {
        text: String,
        /// Accept the reply and write an episode under memory/episodes/
        #[arg(long, group = "verdict")]
        accept: bool,
        /// Reject the reply (still records an episode with verdict=reject)
        #[arg(long, group = "verdict")]
        reject: bool,
        /// Accept an edited reply; value is the corrected text stored as the episode outcome
        #[arg(long, group = "verdict", value_name = "TEXT")]
        edit: Option<String>,
    },
}

#[derive(Subcommand)]
enum CharterCmd {
    Show,
    Set { pack: String },
}

#[derive(Subcommand)]
enum PrefCmd {
    /// List preference names and text
    List,
    /// Add or overwrite a preference (name: lowercase, digits, hyphens)
    Add {
        name: String,
        /// Preference text (short rule)
        text: String,
    },
    /// Delete a preference by name
    Delete { name: String },
}

#[derive(Subcommand)]
enum SecretsCmd {
    /// List secret names and metadata (never values)
    List,
    /// Add a named secret reference. Value is read from --value or stdin.
    Add {
        /// Name: lowercase, digits, hyphens
        name: String,
        /// Secret value (prefer stdin to avoid shell history)
        #[arg(long)]
        value: Option<String>,
        /// Record backend as keychain-stub (still sealed locally in this slice)
        #[arg(long)]
        keychain: bool,
    },
    /// Revoke (delete) a named secret reference and its sealed material
    Revoke { name: String },
}

#[derive(Subcommand)]
enum GrantCmd {
    /// List pending and decided grants from helixd
    List,
    /// Create a pending grant (for testing / future adapters)
    Request {
        /// Action key, e.g. plot.write
        action: String,
        /// Human-readable summary
        summary: String,
        #[arg(long)]
        requester: Option<String>,
    },
    /// Allow a pending grant for a single use
    #[command(name = "allow-once")]
    AllowOnce { id: String },
    /// Allow a pending grant for the current task/session
    #[command(name = "allow-task")]
    AllowTask { id: String },
    /// Deny a pending grant
    Deny { id: String },
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Issue a root token capped by the active charter (JSON on stdout)
    Issue {
        /// Restrict to these rights (comma-separated). Default: full charter set.
        #[arg(long, value_delimiter = ',')]
        rights: Vec<String>,
        /// Optional TTL in seconds
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Attenuate a token to a subset of its rights (cannot widen)
    Attenuate {
        /// Parent token JSON (or path via @file; use - for stdin)
        token: String,
        /// Rights to keep (comma-separated)
        #[arg(long, value_delimiter = ',')]
        keep: Vec<String>,
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Verify a token with helixd
    Verify {
        /// Token JSON (or @file / -)
        token: String,
        /// Require this right
        #[arg(long)]
        require: Option<String>,
    },
    /// Pretty-print token fields without verifying the MAC
    Show {
        /// Token JSON (or @file / -)
        token: String,
    },
}

#[derive(Subcommand)]
enum HandsCmd {
    /// Execute a WASI preview1 Wasm module with only the active plot preopened
    Run {
        /// Path to a .wasm file (core module with optional `_start`)
        wasm: String,
        /// Optional guest argv after the program name
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Optional fuel (instruction budget); omit for unlimited (trusted) or Atlas untrusted cap
        #[arg(long)]
        fuel: Option<u64>,
        /// Plot id under ~/Helix/plots (default: default)
        #[arg(long, default_value = "default")]
        plot: String,
    },
}

#[derive(Subcommand)]
enum AtlasCmd {
    /// List pinned tools (name, digest prefix, path)
    List,
    /// Pin a local tool by SHA-256 of its current bytes
    Pin {
        /// Short name (lowercase, digits, hyphens)
        name: String,
        /// Path to the .wasm (or other tool blob)
        path: String,
    },
    /// Remove a pin by name
    Unpin { name: String },
    /// Verify a pin still matches on-disk bytes
    Verify { name: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { pack } => {
            Charter::builtin(&pack)?;
            let home = HelixHome::resolve()?;
            home.init(&pack)?;
            home.write_pack(&pack)?;
            println!("initialized {}", home.root.display());
            println!("pack {pack}");
        }
        Commands::Status => {
            let url = daemon_url();
            match reqwest::get(format!("{url}/v1/status")).await {
                Ok(res) => {
                    let status: Status = res.error_for_status()?.json().await?;
                    println!("home     {}", status.home);
                    println!("bind     {}", status.bind);
                    println!("pack     {}", status.pack);
                    println!("model    {}", status.model);
                    println!(
                        "ollama   {} reachable={}",
                        status.ollama, status.ollama_reachable
                    );
                    if let Some(ref sw) = status.switch {
                        println!("switch   {}", sw);
                    }
                    if let Some(ref loom) = status.loom {
                        println!("loom     {}", loom);
                    }
                    println!("version  {}", status.version);
                }
                Err(err) => {
                    anyhow::bail!("daemon not reachable at {url} ({err}). Start helixd first.");
                }
            }
        }
        Commands::Charter { action } => {
            let home = HelixHome::resolve()?;
            match action {
                CharterCmd::Show => {
                    let pack = home.read_pack()?;
                    let c = Charter::builtin(&pack)?;
                    println!("pack     {}", c.pack);
                    println!("summary  {}", c.summary);
                    println!("cloud    {}", c.allow_cloud_model);
                    println!("network  {}", c.allow_network_adapters);
                    println!("shell    {}", c.allow_shell);
                    println!("writes_require_ask {}", c.writes_require_ask);
                }
                CharterCmd::Set { pack } => {
                    Charter::builtin(&pack)?;
                    home.init(&pack)?;
                    home.write_pack(&pack)?;
                    println!("pack set to {pack}");
                }
            }
        }
        Commands::Pref { action } => {
            let home = HelixHome::resolve()?;
            home.init(&home.read_pack().unwrap_or_else(|_| DEFAULT_PACK.into()))?;
            match action {
                PrefCmd::List => {
                    let prefs = home.list_prefs()?;
                    if prefs.is_empty() {
                        println!("(no preferences yet)");
                    } else {
                        for p in prefs {
                            println!("{}:\n  {}\n", p.name, p.text.trim());
                        }
                    }
                }
                PrefCmd::Add { name, text } => {
                    let path = home.add_pref(&name, &text)?;
                    println!("wrote {}", path.display());
                }
                PrefCmd::Delete { name } => {
                    home.delete_pref(&name)?;
                    println!("deleted {name}");
                }
            }
        }
        Commands::Secrets { action } => {
            let home = HelixHome::resolve()?;
            home.init(&home.read_pack().unwrap_or_else(|_| DEFAULT_PACK.into()))?;
            let rel = Reliquary::open(home)?;
            match action {
                SecretsCmd::List => {
                    let items = rel.list()?;
                    if items.is_empty() {
                        println!("(no secrets in Reliquary)");
                    } else {
                        for m in items {
                            println!(
                                "{}  backend={}  ref={}  created={}",
                                m.name,
                                match m.backend {
                                    helix_reliquary::Backend::LocalSealed => "local-sealed",
                                    helix_reliquary::Backend::KeychainStub => "keychain-stub",
                                },
                                m.ref_id,
                                m.created_at
                            );
                        }
                    }
                }
                SecretsCmd::Add {
                    name,
                    value,
                    keychain,
                } => {
                    let val = match value {
                        Some(v) => v,
                        None => {
                            use std::io::{self, BufRead};
                            let mut line = String::new();
                            io::stdin().lock().read_line(&mut line)?;
                            line.trim_end_matches(['\r', '\n']).to_string()
                        }
                    };
                    if val.is_empty() {
                        anyhow::bail!("empty secret value");
                    }
                    let meta = rel.add(&name, &val, keychain)?;
                    println!(
                        "sealed {}  backend={}  ref={}",
                        meta.name,
                        match meta.backend {
                            helix_reliquary::Backend::LocalSealed => "local-sealed",
                            helix_reliquary::Backend::KeychainStub => "keychain-stub",
                        },
                        meta.ref_id
                    );
                }
                SecretsCmd::Revoke { name } => {
                    rel.revoke(&name)?;
                    println!("revoked {name}");
                }
            }
        }
        Commands::Grant { action } => {
            let url = daemon_url();
            let client = reqwest::Client::new();
            match action {
                GrantCmd::List => {
                    let res = client
                        .get(format!("{url}/v1/grants"))
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
                    if !res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        anyhow::bail!("list grants failed: {body}");
                    }
                    let body: GrantListResponse = res.json().await?;
                    if body.grants.is_empty() {
                        println!("(no grants)");
                    } else {
                        for g in body.grants {
                            print_grant(&g);
                        }
                    }
                }
                GrantCmd::Request {
                    action,
                    summary,
                    requester,
                } => {
                    let res = client
                        .post(format!("{url}/v1/grants"))
                        .json(&CreateGrantRequest {
                            action,
                            summary,
                            requester,
                        })
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
                    if !res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        anyhow::bail!("create grant failed: {body}");
                    }
                    let g: Grant = res.json().await?;
                    println!("pending grant created");
                    print_grant(&g);
                }
                GrantCmd::AllowOnce { id } => {
                    decide(&client, &url, &id, GrantDecision::AllowOnce).await?;
                }
                GrantCmd::AllowTask { id } => {
                    decide(&client, &url, &id, GrantDecision::AllowTask).await?;
                }
                GrantCmd::Deny { id } => {
                    decide(&client, &url, &id, GrantDecision::Deny).await?;
                }
            }
        }
        Commands::Token { action } => {
            let url = daemon_url();
            let client = reqwest::Client::new();
            match action {
                TokenCmd::Issue { rights, ttl } => {
                    let res = client
                        .post(format!("{url}/v1/tokens"))
                        .json(&IssueTokenRequest {
                            rights,
                            ttl_secs: ttl,
                        })
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
                    if !res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        anyhow::bail!("issue token failed: {body}");
                    }
                    let token: Value = res.json().await?;
                    println!("{}", serde_json::to_string_pretty(&token)?);
                }
                TokenCmd::Attenuate { token, keep, ttl } => {
                    let token_val = read_token_json(&token)?;
                    let res = client
                        .post(format!("{url}/v1/tokens/attenuate"))
                        .json(&AttenuateTokenRequest {
                            token: token_val,
                            keep,
                            ttl_secs: ttl,
                        })
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
                    if !res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        anyhow::bail!("attenuate failed: {body}");
                    }
                    let out: Value = res.json().await?;
                    println!("{}", serde_json::to_string_pretty(&out)?);
                }
                TokenCmd::Verify { token, require } => {
                    let token_val = read_token_json(&token)?;
                    let res = client
                        .post(format!("{url}/v1/tokens/verify"))
                        .json(&VerifyTokenRequest {
                            token: token_val,
                            require,
                        })
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
                    if !res.status().is_success() {
                        let body = res.text().await.unwrap_or_default();
                        anyhow::bail!("verify failed: {body}");
                    }
                    let body: VerifyTokenResponse = res.json().await?;
                    if body.valid {
                        println!("valid");
                        if let Some(r) = body.rights {
                            println!("rights  {}", r.join(", "));
                        }
                    } else {
                        println!("invalid");
                        if let Some(e) = body.error {
                            println!("error   {e}");
                        }
                        std::process::exit(1);
                    }
                }
                TokenCmd::Show { token } => {
                    let token_val = read_token_json(&token)?;
                    if let Some(obj) = token_val.as_object() {
                        if let Some(id) = obj.get("id") {
                            println!("id       {}", id);
                        }
                        if let Some(p) = obj.get("parent") {
                            println!("parent   {}", p);
                        }
                        if let Some(i) = obj.get("issued_at") {
                            println!("issued   {}", i);
                        }
                        if let Some(e) = obj.get("expires_at") {
                            println!("expires  {}", e);
                        }
                        if let Some(r) = obj.get("rights") {
                            println!("rights   {}", r);
                        }
                        if let Some(m) = obj.get("mac") {
                            let s = m.as_str().unwrap_or("");
                            let preview = if s.len() > 16 {
                                format!("{}…", &s[..16])
                            } else {
                                s.to_string()
                            };
                            println!("mac      {preview}");
                        }
                    } else {
                        println!("{}", serde_json::to_string_pretty(&token_val)?);
                    }
                }
            }
        }
        Commands::Hands { action } => match action {
            HandsCmd::Run {
                wasm,
                args,
                fuel,
                plot,
            } => {
                let home = HelixHome::resolve()?;
                home.init(&home.read_pack().unwrap_or_else(|_| DEFAULT_PACK.into()))?;
                let plot_dir = home.root.join("plots").join(&plot);
                if !plot_dir.is_dir() {
                    anyhow::bail!(
                        "plot directory not found: {} (run `helix init` or create plots/{plot})",
                        plot_dir.display()
                    );
                }
                let bytes = std::fs::read(&wasm)
                    .map_err(|e| anyhow::anyhow!("read wasm {}: {e}", wasm))?;
                let atlas = Atlas::open(home.clone());
                let trust = atlas
                    .classify(&bytes, Some(&wasm))
                    .map_err(|e| anyhow::anyhow!("atlas: {e}"))?;
                match &trust {
                    Trust::Trusted { name, sha256 } => {
                        println!(
                            "atlas: trusted pin '{name}' sha256={}…",
                            &sha256[..12.min(sha256.len())]
                        );
                    }
                    Trust::Untrusted {
                        reason,
                        sha256,
                        fuel_cap,
                    } => {
                        eprintln!(
                            "atlas: UNTRUSTED — {reason} (sha256={}…); applying fuel cap {fuel_cap}",
                            &sha256[..12.min(sha256.len())]
                        );
                    }
                }
                let mut guest_args = vec![wasm.clone()];
                guest_args.extend(args);
                let mut cfg = helix_hands::HandsConfig::for_plot(&plot_dir)
                    .with_args(guest_args)
                    .with_inherit_stdio(true);
                // Explicit --fuel always wins; otherwise untrusted tools get the Atlas cap.
                if let Some(f) = fuel {
                    cfg = cfg.with_fuel(f);
                } else if let Trust::Untrusted { fuel_cap, .. } = trust {
                    cfg = cfg.with_fuel(fuel_cap);
                }
                let result = helix_hands::run_module(&bytes, &cfg)
                    .map_err(|e| anyhow::anyhow!("hands: {e}"))?;
                if result.exit_code != 0 {
                    std::process::exit(result.exit_code);
                }
            }
        },
        Commands::Atlas { action } => {
            let home = HelixHome::resolve()?;
            home.init(&home.read_pack().unwrap_or_else(|_| DEFAULT_PACK.into()))?;
            let atlas = Atlas::open(home);
            match action {
                AtlasCmd::List => {
                    let pins = atlas.list().map_err(|e| anyhow::anyhow!("{e}"))?;
                    if pins.is_empty() {
                        println!("(no pins in atlas/pins.json)");
                    } else {
                        for p in pins {
                            let digest = if p.sha256.len() > 12 {
                                format!("{}…", &p.sha256[..12])
                            } else {
                                p.sha256.clone()
                            };
                            println!(
                                "{}  trusted={}  sha256={}  path={}",
                                p.name, p.trusted, digest, p.path
                            );
                        }
                    }
                }
                AtlasCmd::Pin { name, path } => {
                    let pin = atlas
                        .pin(&name, std::path::Path::new(&path))
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!(
                        "pinned {}  sha256={}  path={}",
                        pin.name, pin.sha256, pin.path
                    );
                }
                AtlasCmd::Unpin { name } => {
                    atlas.unpin(&name).map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("unpinned {name}");
                }
                AtlasCmd::Verify { name } => {
                    let pin = atlas
                        .verify_name(&name)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    println!("ok  {}  sha256={}  path={}", pin.name, pin.sha256, pin.path);
                }
            }
        }
        Commands::Ask {
            text,
            accept,
            reject,
            edit,
        } => {
            let url = daemon_url();
            let client = reqwest::Client::new();
            let res = client
                .post(format!("{url}/v1/ask"))
                .json(&AskRequest {
                    text: text.clone(),
                })
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
            if !res.status().is_success() {
                let body = res.text().await.unwrap_or_default();
                anyhow::bail!("ask failed: {body}");
            }
            let body: AskResponse = res.json().await?;
            println!("pack: {}  model_used: {}", body.pack, body.model_used);
            println!("--- memory ---");
            print!("{}", body.memory_context);
            println!("--- reply ---");
            println!("{}", body.reply);

            let verdict = if accept {
                Some(Verdict::Accept)
            } else if reject {
                Some(Verdict::Reject)
            } else if let Some(ref edited) = edit {
                Some(Verdict::Edit)
            } else {
                None
            };

            if let Some(v) = verdict {
                let home = HelixHome::resolve()?;
                let edited_reply = edit.clone();
                let ep = new_episode(
                    &text,
                    &body.reply,
                    edited_reply,
                    v,
                    &body.pack,
                    body.model_used,
                );
                let path = home.write_episode(&ep)?;
                println!("episode {}", path.display());
            }
        }
    }
    Ok(())
}

fn daemon_url() -> String {
    let bind = std::env::var("HELIX_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    format!("http://{bind}")
}

fn print_grant(g: &Grant) {
    println!(
        "{}  {:?}  action={}  {}",
        g.id,
        g.status,
        g.action,
        g.summary
    );
    if let Some(ref s) = g.scope {
        println!("  scope={:?}", s);
    }
    if let Some(ref r) = g.requester {
        println!("  requester={r}");
    }
}

async fn decide(
    client: &reqwest::Client,
    url: &str,
    id: &str,
    decision: GrantDecision,
) -> anyhow::Result<()> {
    let res = client
        .post(format!("{url}/v1/grants/{id}/decide"))
        .json(&DecideGrantRequest { decision })
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("daemon not reachable at {url} ({e})"))?;
    if !res.status().is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("decide failed: {body}");
    }
    let g: Grant = res.json().await?;
    println!("decided");
    print_grant(&g);
    Ok(())
}

fn read_token_json(token: &str) -> anyhow::Result<Value> {
    let raw = if token == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else if let Some(path) = token.strip_prefix('@') {
        std::fs::read_to_string(path)?
    } else {
        token.to_string()
    };
    Ok(serde_json::from_str(raw.trim())?)
}
