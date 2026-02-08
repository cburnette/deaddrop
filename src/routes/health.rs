use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub redis: String,
}

pub async fn health(State(client): State<redis::Client>) -> (StatusCode, Json<HealthResponse>) {
    match client
        .get_connection()
        .and_then(|mut con| redis::cmd("PING").query::<String>(&mut con))
    {
        Ok(_) => (
            StatusCode::OK,
            Json(HealthResponse {
                status: "ok".into(),
                redis: "connected".into(),
            }),
        ),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "degraded".into(),
                redis: e.to_string(),
            }),
        ),
    }
}
