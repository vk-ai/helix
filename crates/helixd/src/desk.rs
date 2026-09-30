use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::Json;
use helix_desk::DeskSnapshot;

use super::App;

pub async fn page() -> Html<&'static str> {
    Html(helix_desk::page_html())
}

pub async fn snapshot(State(app): State<Arc<App>>) -> Json<DeskSnapshot> {
    let grants = {
        let map = app.grants.lock().unwrap();
        let mut g: Vec<_> = map.values().cloned().collect();
        g.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        g
    };
    Json(helix_desk::snapshot(&app.home, &app.bind, grants))
}
