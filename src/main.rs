use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use serde::Serialize;
use std::net::SocketAddr;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    redis: String,
}

async fn health(State(client): State<redis::Client>) -> (StatusCode, Json<HealthResponse>) {
    match client.get_connection().and_then(|mut con| redis::cmd("PING").query::<String>(&mut con)) {
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

#[tokio::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();

    let app = Router::new()
        .route("/health", get(health))
        .with_state(client);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
