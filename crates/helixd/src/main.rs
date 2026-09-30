use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use helix_cap::{rights_for_charter, CapAuthority, CapToken};
use helix_charter::Charter;
use helix_memory::{HelixHome, PREFS_CONTEXT_CHAR_CAP};
use helix_protocol::{
    AskRequest, AskResponse, AttenuateTokenRequest, CreateGrantRequest, DecideGrantRequest,
    ErrorBody, Grant, GrantDecision, GrantListResponse, GrantScope, GrantStatus, IssueTokenRequest,
    Status, VerifyTokenRequest, VerifyTokenResponse, WritePermission, DEFAULT_BIND, DEFAULT_MODEL,
    DEFAULT_OLLAMA,
};
mod desk;
use helix_loom::{self, LoomConfig};
use helix_reliquary::Reliquary;
use helix_switch::Switch;
use serde_json::json;

struct App {
    home: HelixHome,
    bind: String,
    model: String,
    ollama: String,
    /// In-memory pending/allowed grants for this daemon process.
    grants: Mutex<HashMap<String, Grant>>,
    /// Session MAC key for capability tokens (ephemeral; tokens die on restart).
    caps: CapAuthority,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let home = HelixHome::resolve()?;
    let pack = std::env::var("HELIX_PACK")
        .unwrap_or_else(|_| home.read_pack().unwrap_or_else(|_| "hearthside".into()));
    home.init(&pack)?;
    let bind = std::env::var("HELIX_BIND").unwrap_or_else(|_| DEFAULT_BIND.into());
    let model = std::env::var("HELIX_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into());
    let ollama = std::env::var("HELIX_OLLAMA").unwrap_or_else(|_| DEFAULT_OLLAMA.into());
    let app = Arc::new(App {
        home,
        bind: bind.clone(),
        model,
        ollama,
        grants: Mutex::new(HashMap::new()),
        caps: CapAuthority::new_random(),
    });
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/ask", post(ask))
        .route("/v1/grants", get(list_grants).post(create_grant))
        .route("/v1/grants/:id/decide", post(decide_grant))
        .route("/v1/tokens", post(issue_token))
        .route("/v1/tokens/attenuate", post(attenuate_token))
        .route("/v1/tokens/verify", post(verify_token))
        .route("/", get(desk::page))
        .route("/desk", get(desk::page))
        .route("/v1/desk", get(desk::snapshot))
        .with_state(app);
    let addr: SocketAddr = bind.parse()?;
    if !addr.ip().is_loopback() {
        eprintln!("refusing to bind non-loopback address {addr}");
        std::process::exit(2);
    }
    eprintln!("helixd listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

async fn health() -> &'static str {
    "ok\n"
}

fn current_switch(app: &App) -> Switch {
    let pack = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    Switch::for_pack(&pack).unwrap_or_else(|_| {
        Switch::for_pack("hearthside").expect("hearthside always exists")
    })
}

async fn status(State(app): State<Arc<App>>) -> Json<Status> {
    let pack = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let switch = current_switch(&app);
    let ollama_reachable = ollama_ok(&app, &switch).await;
    Json(Status {
        home: app.home.root.display().to_string(),
        bind: app.bind.clone(),
        pack,
        model: app.model.clone(),
        ollama: app.ollama.clone(),
        ollama_reachable,
        version: env!("CARGO_PKG_VERSION").into(),
        switch: Some(switch.summary()),
        loom: None,
    })
}
