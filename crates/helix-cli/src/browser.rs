use helix_browser::{Browser, SessionState};
use helix_memory::HelixHome;
use helix_protocol::DEFAULT_BIND;

use super::BrowserCmd;

pub fn run(cmd: BrowserCmd) -> anyhow::Result<()> {
    let home = HelixHome::resolve()?;
    let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
    Browser::require_desk_plus(&pack)?;
    let browser = Browser::open(home)?;
    match cmd {
        BrowserCmd::Status => println!("{}", browser.status_line()?),
        BrowserCmd::Open { url } => {
            let s = browser.navigate(&url)?;
            if s.state == SessionState::PausedChallenge {
                let kind = s
                    .challenge
                    .as_ref()
                    .map(|c| c.kind.as_str())
                    .unwrap_or("challenge");
                println!(
                    "paused  url={}  kind={}  (human must solve; no solver in core)",
                    s.url, kind
                );
            } else {
                println!("ready  url={}", s.url);
            }
        }
        BrowserCmd::Resume => {
            let s = browser.resume()?;
            println!("resumed  url={}  state={:?}", s.url, s.state);
        }
        BrowserCmd::Pane => {
            let bind = std::env::var("HELIX_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
            println!("http://{bind}/browser");
        }
    }
    Ok(())
}
