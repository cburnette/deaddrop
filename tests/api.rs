use axum_test::TestServer;
use deaddrop::models::{AgentsListResponse, ErrorResponse, RegisterResponse};
use serde_json::json;
use serial_test::serial;

fn test_server() -> TestServer {
    let client = redis::Client::open("redis://127.0.0.1/1").unwrap();

    let mut con = client.get_connection().unwrap();
    redis::cmd("FLUSHDB").exec(&mut con).unwrap();

    TestServer::new(deaddrop::app(client)).unwrap()
}

#[tokio::test]
#[serial]
async fn health_returns_ok() {
    let server = test_server();
    let resp = server.get("/health").await;
    resp.assert_status_ok();
}

#[tokio::test]
#[serial]
async fn register_creates_agent() {
    let server = test_server();

    let resp = server
        .post("/agent/register")
        .json(&json!({"name": "test-agent", "description": "A test agent"}))
        .await;

    resp.assert_status(axum::http::StatusCode::CREATED);

    let body: RegisterResponse = resp.json();
    assert!(body.agent_id.starts_with("dd_"));
    assert_eq!(body.name, "test-agent");
    assert_eq!(body.description, "A test agent");
    assert!(body.active);
}

#[tokio::test]
#[serial]
async fn register_duplicate_name_returns_409() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "dup-agent", "description": "First"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agent/register")
        .json(&json!({"name": "dup-agent", "description": "Second"}))
        .await;

    resp.assert_status(axum::http::StatusCode::CONFLICT);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("already taken"));
}

#[tokio::test]
#[serial]
async fn register_validates_name_too_short() {
    let server = test_server();

    let resp = server
        .post("/agent/register")
        .json(&json!({"name": "ab", "description": "Too short"}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("3-128 characters"));
}

#[tokio::test]
#[serial]
async fn register_validates_name_bad_chars() {
    let server = test_server();

    let resp = server
        .post("/agent/register")
        .json(&json!({"name": "bad name!", "description": "Invalid"}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("alphanumeric"));
}

#[tokio::test]
#[serial]
async fn register_validates_empty_description() {
    let server = test_server();

    let resp = server
        .post("/agent/register")
        .json(&json!({"name": "valid-name", "description": "   "}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial]
async fn list_agents_returns_registered() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "agent-one", "description": "First agent"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/agent/register")
        .json(&json!({"name": "agent-two", "description": "Second agent"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server.get("/agents").await;
    resp.assert_status_ok();

    let body: AgentsListResponse = resp.json();
    assert_eq!(body.agents.len(), 2);

    let names: Vec<&str> = body.agents.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"agent-one"));
    assert!(names.contains(&"agent-two"));
}

#[tokio::test]
#[serial]
async fn list_agents_empty_when_none_registered() {
    let server = test_server();

    let resp = server.get("/agents").await;
    resp.assert_status_ok();

    let body: AgentsListResponse = resp.json();
    assert!(body.agents.is_empty());
}
