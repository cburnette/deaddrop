pub mod auth;
pub mod models;
pub mod routes;

use axum::routing::{get, post};
use axum::Router;

/// Create the RediSearch index for agents (idempotent — ignores "Index already exists").
pub fn ensure_search_index(client: &redis::Client) {
    let Ok(mut con) = client.get_connection() else {
        eprintln!("WARNING: could not connect to Redis to create search index");
        return;
    };

    let result: Result<String, redis::RedisError> = redis::cmd("FT.CREATE")
        .arg("idx:agents")
        .arg("ON")
        .arg("HASH")
        .arg("PREFIX")
        .arg("1")
        .arg("agent:")
        .arg("SCHEMA")
        .arg("name")
        .arg("TEXT")
        .arg("description")
        .arg("TEXT")
        .arg("active")
        .arg("TAG")
        .query(&mut con);

    match result {
        Ok(_) => {}
        Err(e) if e.to_string().contains("Index already exists") => {}
        Err(e) => eprintln!("WARNING: failed to create search index: {e}"),
    }
}

pub fn app(client: redis::Client) -> Router {
    ensure_search_index(&client);

    Router::new()
        .route("/health", get(routes::health::health))
        .route("/agent/register", post(routes::register::register))
        .route("/agents/search", post(routes::search::search))
        .route("/messages/send", post(routes::messages::send))
        .route("/messages", get(routes::messages::poll))
        .route("/agent/deactivate", post(routes::agent::deactivate))
        .route("/agent/activate", post(routes::agent::activate))
        .route("/admin/stats", get(routes::admin::stats))
        .with_state(client)
}
