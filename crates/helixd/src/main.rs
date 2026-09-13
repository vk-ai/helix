use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use helix_charter::Charter;
use helix_memory::HelixHome;
use helix_protocol::{
    AskRequest, AskResponse, ErrorBody, Status, DEFAULT_BIND, DEFAULT_MODEL, DEFAULT_OLLAMA,
};
use serde::Deserialize;
use serde_json::json;

struct App {
    home: HelixHome,
    bind: String,
    model: String,
    ollama: String,
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
    });

    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/ask", post(ask))
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

async fn status(State(app): State<Arc<App>>) -> Json<Status> {
    let pack = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let ollama_reachable = ollama_ok(&app.ollama).await;
    Json(Status {
        home: app.home.root.display().to_string(),
        bind: app.bind.clone(),
        pack,
        model: app.model.clone(),
        ollama: app.ollama.clone(),
        ollama_reachable,
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn ask(
    State(app): State<Arc<App>>,
    Json(req): Json<AskRequest>,
) -> Result<Json<AskResponse>, (StatusCode, Json<ErrorBody>)> {
    let pack_name = app
        .home
        .read_pack()
        .unwrap_or_else(|_| "hearthside".into());
    let charter = Charter::builtin(&pack_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let memory = app
        .home
        .retrieve_context(&req.text)
        .unwrap_or_else(|_| "(memory unavailable)\n".into());

    let (reply, model_used) = match loom_complete(&app, &charter, &memory, &req.text).await {
        Ok(text) => (text, true),
        Err(err) => (
            format!(
                "Charter ({pack_name}): {}\n\nMemory:\n{memory}\nOllama was not used ({err}).",
                charter.summary
            ),
            false,
        ),
    };

    let _ = app.home.append_chronicle(
        &json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "kind": "ask",
            "pack": pack_name,
            "model_used": model_used,
        })
        .to_string(),
    );

    Ok(Json(AskResponse {
        pack: pack_name,
        memory_context: memory,
        reply,
        model_used,
    }))
}

async fn ollama_ok(base: &str) -> bool {
    reqwest::Client::new()
        .get(format!("{base}/api/tags"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: Option<String>,
}

async fn loom_complete(
    app: &App,
    charter: &Charter,
    memory: &str,
    user: &str,
) -> anyhow::Result<String> {
    if !charter.allow_local_model {
        anyhow::bail!("local model not allowed by charter");
    }
    let prompt = format!(
        "You are Helix, a local personal agent.\n\
         You have no tools and no secrets in this slice.\n\
         Charter pack: {}\n{}\n\n\
         Retrieved memory:\n{}\n\n\
         User:\n{}\n\n\
         Reply helpfully. Do not invent capabilities you do not have.\n",
        charter.pack, charter.summary, memory, user
    );
    let body = json!({
        "model": app.model,
        "prompt": prompt,
        "stream": false,
    });
    let res = reqwest::Client::new()
        .post(format!("{}/api/generate", app.ollama))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: OllamaResponse = res.json().await?;
    Ok(parsed
        .response
        .unwrap_or_else(|| "(empty model response)".into()))
}
