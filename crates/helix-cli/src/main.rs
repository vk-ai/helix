use clap::{Parser, Subcommand};
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
            } else if edit.is_some() {
                Some(Verdict::Edit)
            } else {
                None
            };

            if let Some(verdict) = verdict {
                let home = HelixHome::resolve()?;
                home.init(&body.pack)?;
                let episode = new_episode(
                    &text,
                    &body.reply,
                    edit.clone(),
                    verdict,
                    &body.pack,
                    body.model_used,
                );
                let path = home.write_episode(&episode)?;
                println!("--- episode ---");
                println!("verdict  {:?}", verdict);
                println!("wrote    {}", path.display());
                if matches!(verdict, Verdict::Accept | Verdict::Edit) {
                    println!(
                        "(playbook may appear under memory/playbooks/ after two similar successes)"
                    );
                }
            }
        }
    }
    Ok(())
}

fn read_token_json(spec: &str) -> anyhow::Result<Value> {
    let raw = if spec == "-" {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else if let Some(path) = spec.strip_prefix('@') {
        std::fs::read_to_string(path)?
    } else {
        spec.to_string()
    };
    Ok(serde_json::from_str(raw.trim())?)
}

fn print_grant(g: &Grant) {
    let scope = g
        .scope
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| "-".into());
    println!(
        "{}  status={:?}  scope={}  action={}  {}",
        g.id, g.status, scope, g.action, g.summary
    );
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
    println!("grant updated");
    print_grant(&g);
    Ok(())
}

fn daemon_url() -> String {
    let bind = std::env::var("HELIX_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    format!("http://{bind}")
}
