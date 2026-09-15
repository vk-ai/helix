use clap::{Parser, Subcommand};
use helix_charter::Charter;
use helix_memory::{new_episode, HelixHome, Verdict};
use helix_protocol::{AskRequest, AskResponse, Status, DEFAULT_BIND, DEFAULT_PACK};

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
                }
                CharterCmd::Set { pack } => {
                    Charter::builtin(&pack)?;
                    home.init(&pack)?;
                    home.write_pack(&pack)?;
                    println!("pack set to {pack}");
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

fn daemon_url() -> String {
    let bind = std::env::var("HELIX_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    format!("http://{bind}")
}
