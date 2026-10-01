use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Html;
use axum::Json;
use helix_browser::{Browser, Session};
use helix_protocol::ErrorBody;
use serde::Serialize;

use super::App;

#[derive(Serialize)]
pub struct BrowserSnapshot {
    pub pack: String,
    pub profile: String,
    pub url: String,
    pub state: helix_browser::SessionState,
    pub challenge: Option<helix_browser::Challenge>,
    pub history: Vec<String>,
}

pub async fn page() -> Html<&'static str> {
    Html(helix_browser::pane_html())
}

fn load(app: &App) -> Result<(Browser, Session, String), (StatusCode, Json<ErrorBody>)> {
    let pack = app.home.read_pack().unwrap_or_else(|_| "hearthside".into());
    Browser::require_desk_plus(&pack).map_err(|e| {
        (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let browser = Browser::open(app.home.clone()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    let session = browser.session().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    Ok((browser, session, pack))
}

pub async fn snapshot(
    State(app): State<Arc<App>>,
) -> Result<Json<BrowserSnapshot>, (StatusCode, Json<ErrorBody>)> {
    let (browser, session, pack) = load(&app)?;
    Ok(Json(BrowserSnapshot {
        pack,
        profile: browser.profile_dir().display().to_string(),
        url: session.url,
        state: session.state,
        challenge: session.challenge,
        history: session.history,
    }))
}

pub async fn resume(
    State(app): State<Arc<App>>,
) -> Result<Json<BrowserSnapshot>, (StatusCode, Json<ErrorBody>)> {
    let (browser, _, pack) = load(&app)?;
    let session = browser.resume().map_err(|e| {
        (
            StatusCode::CONFLICT,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
    })?;
    Ok(Json(BrowserSnapshot {
        pack,
        profile: browser.profile_dir().display().to_string(),
        url: session.url,
        state: session.state,
        challenge: session.challenge,
        history: session.history,
    }))
}
