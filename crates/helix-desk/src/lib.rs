//! Loopback desk page: Ask banners, Reliquary names, Chronicle tail.
//!
//! Served by helixd on loopback only. Never includes secret values.

use helix_memory::HelixHome;
use helix_protocol::Grant;
use helix_reliquary::{Reliquary, SecretMeta};
use serde::{Deserialize, Serialize};

pub const DESK_PATH: &str = "/desk";
pub const DESK_API_PATH: &str = "/v1/desk";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeskSecret {
    pub name: String,
    pub backend: String,
    pub ref_id: String,
}

impl From<&SecretMeta> for DeskSecret {
    fn from(m: &SecretMeta) -> Self {
        Self {
            name: m.name.clone(),
            backend: format!("{:?}", m.backend),
            ref_id: m.ref_id.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeskSnapshot {
    pub pack: String,
    pub bind: String,
    pub home: String,
    pub grants: Vec<Grant>,
    pub secrets: Vec<DeskSecret>,
    pub chronicle: Vec<String>,
}

pub fn page_html() -> &'static str {
    PAGE
}

/// Last `n` chronicle lines (raw JSONL). Never includes Reliquary values.
pub fn chronicle_tail(home: &HelixHome, n: usize) -> Vec<String> {
    let path = home.root.join("chronicle/log.jsonl");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|s| s.to_string())
        .collect();
    if lines.len() > n {
        lines = lines.split_off(lines.len() - n);
    }
    lines
}

pub fn snapshot(home: &HelixHome, bind: &str, grants: Vec<Grant>) -> DeskSnapshot {
    let pack = home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let secrets = Reliquary::open(home.clone())
        .ok()
        .and_then(|r| r.list().ok())
        .unwrap_or_default()
        .iter()
        .map(DeskSecret::from)
        .collect();
    DeskSnapshot {
        pack,
        bind: bind.to_string(),
        home: home.root.display().to_string(),
        grants,
        secrets,
        chronicle: chronicle_tail(home, 40),
    }
}

const PAGE: &str = r#"<!DOCTYPE html>
<html lang=\"en\"><head><meta charset=\"utf-8\"/><title>Helix desk</title>
<style>body{font-family:sans-serif;background:#111;color:#eee;margin:1rem}section{border:1px solid #333;padding:1rem;margin:1rem 0}button{margin:.25rem} .empty{color:#666}</style>
</head><body>
<h1>Helix desk</h1><p id=\"hdr\">loopback</p>
<section><h2>Ask banners</h2><div id=\"grants\"></div></section>
<section><h2>Reliquary names</h2><div id=\"secrets\"></div></section>
<section><h2>Chronicle tail</h2><pre id=\"log\"></pre></section>
<script>
async function decide(id, decision) {
  await fetch('/v1/grants/' + encodeURIComponent(id) + '/decide', {method:'POST', headers:{'content-type':'application/json'}, body: JSON.stringify({decision})});
  load();
}
async function load() {
  const res = await fetch('/v1/desk');
  if (!res.ok) { document.getElementById('hdr').textContent = 'desk api failed'; return; }
  const d = await res.json();
  document.getElementById('hdr').textContent = 'pack=' + d.pack + ' bind=' + d.bind;
  const g = (d.grants || []).filter(x => x.status === 'pending');
  const ge = document.getElementById('grants');
  ge.innerHTML = g.length ? g.map(x => '<div><b>'+x.action+'</b> '+x.summary+' <code>'+x.id+'</code><br/>'+'<button onclick=\"decide(\''+x.id+'\',\'allow-once\')\">allow-once</button> <button onclick=\"decide(\''+x.id+'\',\'allow-task\')\">allow-task</button> <button onclick=\"decide(\''+x.id+'\',\'deny\')\">deny</button></div>').join('') : '<p class=empty>No pending grants.</p>';
  const s = d.secrets || [];
  document.getElementById('secrets').innerHTML = s.length ? s.map(x => '<div><b>'+x.name+'</b> backend='+x.backend+' ref='+x.ref_id+'</div>').join('') : '<p class=empty>No named secrets (values never shown).</p>';
  document.getElementById('log').textContent = (d.chronicle || []).join('\n') || '(empty chronicle)';
}
load(); setInterval(load, 3000);
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_mentions_banners_and_reliquary() {
        let html = page_html();
        assert!(html.contains("Ask banners"));
        assert!(html.contains("Reliquary names"));
        assert!(html.contains("Chronicle tail"));
        assert!(html.contains("/v1/desk"));
        assert!(!html.contains("password"));
    }
}
