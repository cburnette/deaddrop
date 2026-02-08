use axum::{extract::Query, extract::State, routing::get, Json, Router};
use redis::Commands;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Deserialize)]
struct PingParams {
    value: String,
}

#[derive(Serialize)]
struct PingResponse {
    value: String,
}

async fn ping(
    State(client): State<redis::Client>,
    Query(params): Query<PingParams>,
) -> Json<PingResponse> {
    let mut con = client.get_connection().unwrap();

    let _: () = con.set("ping", &params.value).unwrap();
    let result: String = con.get("ping").unwrap();

    Json(PingResponse { value: result })
}

#[tokio::main]
async fn main() {
    let client = redis::Client::open("redis://127.0.0.1/").unwrap();

    let app = Router::new()
        .route("/ping", get(ping))
        .with_state(client);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
