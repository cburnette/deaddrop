mod models;
mod routes;

use axum::routing::{get, post};
use axum::Router;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();

    let app = Router::new()
        .route("/health", get(routes::health::health))
        .route("/agent/register", post(routes::register::register))
        .route("/agents", get(routes::agents::list_agents))
        .with_state(client);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
