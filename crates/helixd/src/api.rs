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

pub async fn list_grants(State(app): State<Arc<App>>) -> Json<GrantListResponse> {
    let map = app.grants.lock().unwrap();
    let mut grants: Vec<Grant> = map.values().cloned().collect();
    grants.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Json(GrantListResponse { grants })
}

pub async fn create_grant(
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
    let _ = Charter::builtin(&pack_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
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

pub async fn decide_grant(
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

pub async fn issue_token(
    State(app): State<Arc<App>>,
    Json(req): Json<IssueTokenRequest>,
) -> Result<Json<CapToken>, (StatusCode, Json<ErrorBody>)> {
    let pack_name = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    let charter = Charter::builtin(&pack_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let max = rights_for_charter(
        charter.allow_local_model,
        charter.allow_cloud_model,
        charter.allow_network_adapters,
        charter.allow_shell,
    );
    let rights: BTreeSet<String> = if req.rights.is_empty() {
        max.clone()
    } else {
        let requested: BTreeSet<String> = req.rights.into_iter().collect();
        if !requested.is_subset(&max) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorBody {
                    error: format!(
                        "requested rights exceed charter {pack_name}; allowed: {}",
                        max.iter().cloned().collect::<Vec<_>>().join(", ")
                    ),
                }),
            ));
        }
        requested
    };
    let token = app.caps.issue(rights, req.ttl_secs).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let _ = app.home.append_chronicle(
        &json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "kind": "token_issue",
            "id": token.id,
            "rights": token.rights.iter().cloned().collect::<Vec<_>>(),
        })
        .to_string(),
    );
    Ok(Json(token))
}

pub async fn attenuate_token(
    State(app): State<Arc<App>>,
    Json(req): Json<AttenuateTokenRequest>,
) -> Result<Json<CapToken>, (StatusCode, Json<ErrorBody>)> {
    let parent: CapToken = serde_json::from_value(req.token).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: format!("invalid token json: {e}"),
            }),
        )
    })?;
    let keep: BTreeSet<String> = req.keep.into_iter().collect();
    let child = app.caps.attenuate(&parent, keep, req.ttl_secs).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let _ = app.home.append_chronicle(
        &json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "kind": "token_attenuate",
            "id": child.id,
            "parent": child.parent,
            "rights": child.rights.iter().cloned().collect::<Vec<_>>(),
        })
        .to_string(),
    );
    Ok(Json(child))
}

pub async fn verify_token(
    State(app): State<Arc<App>>,
    Json(req): Json<VerifyTokenRequest>,
) -> Json<VerifyTokenResponse> {
    let token: CapToken = match serde_json::from_value(req.token) {
        Ok(t) => t,
        Err(e) => {
            return Json(VerifyTokenResponse {
                valid: false,
                error: Some(format!("invalid token json: {e}")),
                rights: None,
            });
        }
    };
    match app.caps.verify(&token) {
        Ok(()) => {
            if let Some(ref right) = req.require {
                if !token.rights.contains(right) {
                    return Json(VerifyTokenResponse {
                        valid: false,
                        error: Some(format!("missing right: {right}")),
                        rights: Some(token.rights.iter().cloned().collect()),
                    });
                }
            }
            Json(VerifyTokenResponse {
                valid: true,
                error: None,
                rights: Some(token.rights.iter().cloned().collect()),
            })
        }
        Err(e) => Json(VerifyTokenResponse {
            valid: false,
            error: Some(e.to_string()),
            rights: None,
        }),
    }
}

#[allow(dead_code)]
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

async fn loom_complete(
    app: &App,
    charter: &Charter,
    switch: &Switch,
    memory: &str,
    user: &str,
) -> anyhow::Result<String> {
    let config = LoomConfig::from_env(&app.model, &app.ollama);
    let prompt = helix_loom::build_ask_prompt(charter, memory, user);
    let reliquary = Reliquary::open(app.home.clone()).ok();
    let out = helix_loom::complete(charter, switch, &config, reliquary.as_ref(), &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(out.text)
}
