use helix_apprentice::Apprentice;
use helix_memory::HelixHome;

use super::ApprenticeCmd;

pub fn run(cmd: ApprenticeCmd) -> anyhow::Result<()> {
    let home = HelixHome::resolve()?;
    let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
    Apprentice::require_workshop(&pack)?;
    let apprentice = Apprentice::open(home)?;
    match cmd {
        ApprenticeCmd::Status => println!("{}", apprentice.status_line()?),
        ApprenticeCmd::Propose { task } => {
            for p in apprentice.propose(&task)? {
                println!("{}\tscore={}\t{}", p.tool, p.score, p.note);
            }
        }
        ApprenticeCmd::Execute { tool, input } => {
            let t = apprentice.execute(&tool, &input)?;
            println!("{}  ok={}  output={}", t.tool, t.ok, t.output);
        }
        ApprenticeCmd::Select { tool } => {
            let path = apprentice.select(&tool)?;
            println!("note {}", path.display());
        }
    }
    Ok(())
}
