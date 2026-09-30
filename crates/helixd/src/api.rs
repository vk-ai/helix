use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use helix_cap::{rights_for_charter, CapToken};
use helix_charter::Charter;
use helix_loom::LoomConfig;
use helix_memory::PREFS_CONTEXT_CHAR_CAP;
use helix_protocol::{
    AskRequest, AskResponse, AttenuateTokenRequest, CreateGrantRequest, DecideGrantRequest,
    ErrorBody, Grant, GrantDecision, GrantListResponse, GrantScope, GrantStatus, IssueTokenRequest,
    VerifyTokenRequest, VerifyTokenResponse, WritePermission,
};
use helix_reliquary::Reliquary;
use helix_switch::Switch;
use serde_json::json;

use super::App;

pub async fn ask(
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
    let memory = if prefs.is_empty() {
        retrieved
    } else {
        format!("{prefs}\n{retrieved}")
    };
    let switch = Switch::from_charter(&charter);
    let loom = loom_complete(&app, &charter, &switch, &memory, &req.text).await;
    let (reply, model_used) = match loom {
        Ok(text) => (text, true),
        Err(err) => (
            format!(
                "Charter ({pack_name}): {}\n\nMemory:\n{memory}\nLoom was not used ({err}).",
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
