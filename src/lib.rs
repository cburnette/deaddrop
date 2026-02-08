pub mod models;
pub mod routes;

use axum::routing::{get, post};
use axum::Router;

pub fn app(client: redis::Client) -> Router {
    Router::new()
        .route("/health", get(routes::health::health))
        .route("/agent/register", post(routes::register::register))
        .route("/agents", get(routes::agents::list_agents))
        .with_state(client)
}
