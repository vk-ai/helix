use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use helix_charter::Charter;
use helix_memory::{HelixHome, PREFS_CONTEXT_CHAR_CAP};
use helix_protocol::{
    AskRequest, AskResponse, CreateGrantRequest, DecideGrantRequest, ErrorBody, Grant,
    GrantDecision, GrantListResponse, GrantScope, GrantStatus, Status, WritePermission,
    DEFAULT_BIND, DEFAULT_MODEL, DEFAULT_OLLAMA,
};
use serde::Deserialize;
use serde_json::json;

struct App {
    home: HelixHome,
    bind: String,
    model: String,
    ollama: String,
    /// In-memory pending/allowed grants for this daemon process.
    grants: Mutex<HashMap<String, Grant>>,
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
    });

    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/ask", post(ask))
        .route("/v1/grants", get(list_grants).post(create_grant))
        .route("/v1/grants/:id/decide", post(decide_grant))
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
    let pack_name = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let charter = Charter::builtin(&pack_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;

    let prefs = app
        .home
        .prefs_context(PREFS_CONTEXT_CHAR_CAP)
        .unwrap_or_default();
    let retrieved = app
        .home
        .retrieve_context(&req.text)
        .unwrap_or_else(|_| "(memory unavailable)\n".into());

    // Prefs first (capped), then keyword hits from episodes/playbooks/tools.
    let memory = if prefs.is_empty() {
        retrieved
    } else {
        format!("{prefs}\n{retrieved}")
    };

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

async fn list_grants(State(app): State<Arc<App>>) -> Json<GrantListResponse> {
    let map = app.grants.lock().unwrap();
    let mut grants: Vec<Grant> = map.values().cloned().collect();
    grants.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Json(GrantListResponse { grants })
}

async fn create_grant(
    State(app): State<Arc<App>>,
    Json(req): Json<CreateGrantRequest>,
) -> Result<Json<Grant>, (StatusCode, Json<ErrorBody>)> {
    let action = req.action.trim();
    let summary = req.summary.trim();
    if action.is_empty() || summary.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "action and summary are required".into(),
            }),
        ));
    }
    if action.len() > 128 || summary.len() > 512 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "action or summary too long".into(),
            }),
        ));
    }

    let pack_name = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let charter = Charter::builtin(&pack_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;

    // When writes_require_ask is false, adapters may still record grants for audit,
    // but the check helper will treat writes as allowed without a grant.
    let _ = charter.writes_require_ask;

    let id = format!("g-{}", chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
    let grant = Grant {
        id: id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        action: action.to_string(),
        summary: summary.to_string(),
        requester: req.requester.filter(|s| !s.trim().is_empty()),
        status: GrantStatus::Pending,
        scope: None,
        decided_at: None,
    };

    {
        let mut map = app.grants.lock().unwrap();
        map.insert(id.clone(), grant.clone());
    }

    let _ = app.home.append_chronicle(
        &json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "kind": "grant_pending",
            "id": id,
            "action": grant.action,
            "summary": grant.summary,
        })
        .to_string(),
    );

    Ok(Json(grant))
}

async fn decide_grant(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(req): Json<DecideGrantRequest>,
) -> Result<Json<Grant>, (StatusCode, Json<ErrorBody>)> {
    let mut map = app.grants.lock().unwrap();
    let grant = map.get_mut(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorBody {
                error: format!("grant not found: {id}"),
            }),
        )
    })?;

    if grant.status != GrantStatus::Pending {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: format!("grant {id} is already {:?}", grant.status),
            }),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    match req.decision {
        GrantDecision::AllowOnce => {
            grant.status = GrantStatus::Allowed;
            grant.scope = Some(GrantScope::Once);
            grant.decided_at = Some(now.clone());
        }
        GrantDecision::AllowTask => {
            grant.status = GrantStatus::Allowed;
            grant.scope = Some(GrantScope::Task);
            grant.decided_at = Some(now.clone());
        }
        GrantDecision::Deny => {
            grant.status = GrantStatus::Denied;
            grant.scope = None;
            grant.decided_at = Some(now.clone());
        }
    }

    let out = grant.clone();
    drop(map);

    let _ = app.home.append_chronicle(
        &json!({
            "ts": now,
            "kind": "grant_decision",
            "id": out.id,
            "action": out.action,
            "decision": format!("{:?}", req.decision),
            "status": format!("{:?}", out.status),
        })
        .to_string(),
    );

    Ok(Json(out))
}

/// Enforce `writes_require_ask` for future adapters.
///
/// Returns whether the write may proceed, needs a new grant, or should wait on a pending one.
/// When a Once grant is used, it is marked Consumed.
pub fn check_write_permission(app: &App, action: &str) -> WritePermission {
    let pack_name = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let charter = match Charter::builtin(&pack_name) {
        Ok(c) => c,
        Err(_) => return WritePermission::NeedsGrant,
    };

    if !charter.writes_require_ask {
        return WritePermission::Allowed;
    }

    let mut map = app.grants.lock().unwrap();
    let mut pending = false;
    let mut allowed_id: Option<String> = None;
    let mut once = false;

    for g in map.values() {
        if g.action != action {
            continue;
        }
        match g.status {
            GrantStatus::Pending => pending = true,
            GrantStatus::Allowed => {
                allowed_id = Some(g.id.clone());
                once = matches!(g.scope, Some(GrantScope::Once));
                break;
            }
            GrantStatus::Denied | GrantStatus::Consumed => {}
        }
    }

    if let Some(id) = allowed_id {
        if once {
            if let Some(g) = map.get_mut(&id) {
                g.status = GrantStatus::Consumed;
            }
        }
        return WritePermission::Allowed;
    }

    if pending {
        WritePermission::PendingExists
    } else {
        WritePermission::NeedsGrant
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use helix_protocol::WritePermission;

    #[test]
    fn writes_require_ask_blocks_without_grant() {
        let home = HelixHome {
            root: std::env::temp_dir().join(format!(
                "helixd-grant-test-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
        };
        home.init("hearthside").unwrap();
        home.write_pack("hearthside").unwrap();
        let app = App {
            home,
            bind: DEFAULT_BIND.into(),
            model: DEFAULT_MODEL.into(),
            ollama: DEFAULT_OLLAMA.into(),
            grants: Mutex::new(HashMap::new()),
        };
        assert_eq!(
            check_write_permission(&app, "plot.write"),
            WritePermission::NeedsGrant
        );
        let _ = std::fs::remove_dir_all(&app.home.root);
    }

    #[test]
    fn allow_once_then_consume() {
        let home = HelixHome {
            root: std::env::temp_dir().join(format!(
                "helixd-grant-once-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )),
        };
        home.init("hearthside").unwrap();
        home.write_pack("hearthside").unwrap();
        let app = App {
            home,
            bind: DEFAULT_BIND.into(),
            model: DEFAULT_MODEL.into(),
            ollama: DEFAULT_OLLAMA.into(),
            grants: Mutex::new(HashMap::new()),
        };
        {
            let mut map = app.grants.lock().unwrap();
            map.insert(
                "g-1".into(),
                Grant {
                    id: "g-1".into(),
                    created_at: "t".into(),
                    action: "plot.write".into(),
                    summary: "write file".into(),
                    requester: None,
                    status: GrantStatus::Allowed,
                    scope: Some(GrantScope::Once),
                    decided_at: Some("t".into()),
                },
            );
        }
        assert_eq!(
            check_write_permission(&app, "plot.write"),
            WritePermission::Allowed
        );
        // Second check: Once grant was consumed.
        assert_eq!(
            check_write_permission(&app, "plot.write"),
            WritePermission::NeedsGrant
        );
        let _ = std::fs::remove_dir_all(&app.home.root);
    }
}
