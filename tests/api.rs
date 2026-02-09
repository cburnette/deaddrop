use axum_test::TestServer;
use deaddrop::models::{ErrorResponse, RegisterResponse, SearchResponse, SendMessageResponse};
use serde_json::json;
use serial_test::serial;

fn test_server() -> TestServer {
    // RediSearch only works on DB 0, so tests must use DB 0
    let client = redis::Client::open("redis://127.0.0.1/0").unwrap();

    let mut con = client.get_connection().unwrap();
    redis::cmd("FLUSHDB").exec(&mut con).unwrap();

    // Recreate search index after FLUSHDB (which destroys indexes)
    deaddrop::ensure_search_index(&client);

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
    assert!(body.api_key.starts_with("dd_key_"));
    assert_eq!(body.api_key.len(), 71);
    assert_eq!(body.name, "test-agent");
    assert_eq!(body.description, "A test agent");
    assert!(body.active);
}

#[tokio::test]
#[serial]
async fn register_returns_unique_api_keys() {
    let server = test_server();

    let resp1: RegisterResponse = server
        .post("/agent/register")
        .json(&json!({"name": "agent-one", "description": "First"}))
        .await
        .json();

    let resp2: RegisterResponse = server
        .post("/agent/register")
        .json(&json!({"name": "agent-two", "description": "Second"}))
        .await
        .json();

    assert_ne!(resp1.api_key, resp2.api_key);
}

#[tokio::test]
#[serial]
async fn register_api_key_resolves_to_agent_id() {
    let server = test_server();

    let body: RegisterResponse = server
        .post("/agent/register")
        .json(&json!({"name": "auth-test", "description": "Testing auth"}))
        .await
        .json();

    // Verify the reverse index exists by looking up the hash
    let client = redis::Client::open("redis://127.0.0.1/0").unwrap();
    let hash = deaddrop::auth::hash_token(&body.api_key);
    let mut con = client.get_connection().unwrap();
    let stored_id: String = redis::Commands::get(&mut con, format!("auth:{hash}")).unwrap();
    assert_eq!(stored_id, body.agent_id);
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

// --- Search tests ---

#[tokio::test]
#[serial]
async fn search_returns_matching_agents() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "weather-bot", "description": "Provides weather forecasting and climate data"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/agent/register")
        .json(&json!({"name": "stock-bot", "description": "Tracks stock market prices"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": ["weather forecasting"]}))
        .await;

    resp.assert_status_ok();

    let body: SearchResponse = resp.json();
    assert_eq!(body.results.len(), 1);
    assert_eq!(body.results[0].name, "weather-bot");
}

#[tokio::test]
#[serial]
async fn search_multiple_phrases_returns_union() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "weather-bot", "description": "Provides weather forecasting"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/agent/register")
        .json(&json!({"name": "stock-bot", "description": "Tracks stock market prices"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    server
        .post("/agent/register")
        .json(&json!({"name": "news-bot", "description": "Aggregates news articles"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": ["weather", "stock"]}))
        .await;

    resp.assert_status_ok();

    let body: SearchResponse = resp.json();
    assert_eq!(body.results.len(), 2);

    let names: Vec<&str> = body.results.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"weather-bot"));
    assert!(names.contains(&"stock-bot"));
}

#[tokio::test]
#[serial]
async fn search_no_matches_returns_empty() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "weather-bot", "description": "Provides weather data"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": ["cryptocurrency"]}))
        .await;

    resp.assert_status_ok();

    let body: SearchResponse = resp.json();
    assert!(body.results.is_empty());
}

#[tokio::test]
#[serial]
async fn search_empty_phrases_returns_400() {
    let server = test_server();

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": []}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-10"));
}

#[tokio::test]
#[serial]
async fn search_too_many_phrases_returns_400() {
    let server = test_server();

    let phrases: Vec<String> = (0..11).map(|i| format!("phrase{i}")).collect();

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": phrases}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-10"));
}

#[tokio::test]
#[serial]
async fn search_phrase_too_long_returns_400() {
    let server = test_server();

    let long_phrase = "a".repeat(257);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": [long_phrase]}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-256"));
}

#[tokio::test]
#[serial]
async fn search_matches_name_field() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "climate-analyzer", "description": "General purpose tool"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": ["climate"]}))
        .await;

    resp.assert_status_ok();

    let body: SearchResponse = resp.json();
    assert_eq!(body.results.len(), 1);
    assert_eq!(body.results[0].name, "climate-analyzer");
}

#[tokio::test]
#[serial]
async fn search_matches_description_field() {
    let server = test_server();

    server
        .post("/agent/register")
        .json(&json!({"name": "helper-bot", "description": "Specializes in oceanography research"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .post("/agents/search")
        .json(&json!({"phrases": ["oceanography"]}))
        .await;

    resp.assert_status_ok();

    let body: SearchResponse = resp.json();
    assert_eq!(body.results.len(), 1);
    assert_eq!(body.results[0].name, "helper-bot");
}

// --- Message tests ---

async fn register_agent(server: &TestServer, name: &str, desc: &str) -> RegisterResponse {
    server
        .post("/agent/register")
        .json(&json!({"name": name, "description": desc}))
        .await
        .json()
}

#[tokio::test]
#[serial]
async fn send_message_single_recipient_succeeds() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recipient.agent_id], "body": "Hello there!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::CREATED);

    let body: SendMessageResponse = resp.json();
    assert!(body.message_id.starts_with("msg_"));
    assert_eq!(body.from, sender.agent_id);
    assert_eq!(body.to, vec![recipient.agent_id]);
}

#[tokio::test]
#[serial]
async fn send_message_multiple_recipients_succeeds() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recv1 = register_agent(&server, "recv-one", "First recipient").await;
    let recv2 = register_agent(&server, "recv-two", "Second recipient").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recv1.agent_id, recv2.agent_id], "body": "Hello everyone!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::CREATED);

    let body: SendMessageResponse = resp.json();
    assert_eq!(body.to.len(), 2);
}

#[tokio::test]
#[serial]
async fn send_message_without_auth_returns_401() {
    let server = test_server();
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let resp = server
        .post("/messages/send")
        .json(&json!({"to": [recipient.agent_id], "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn send_message_invalid_token_returns_401() {
    let server = test_server();
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            "Bearer dd_key_invalid".parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recipient.agent_id], "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn send_message_nonexistent_recipient_returns_404() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": ["dd_nonexistent"], "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn send_message_to_self_returns_403() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [sender.agent_id], "body": "Talking to myself"}))
        .await;

    resp.assert_status(axum::http::StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial]
async fn send_message_empty_body_returns_400() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recipient.agent_id], "body": "   "}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-32768"));
}

#[tokio::test]
#[serial]
async fn send_message_body_too_long_returns_400() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let long_body = "a".repeat(32769);

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recipient.agent_id], "body": long_body}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-32768"));
}

#[tokio::test]
#[serial]
async fn send_message_empty_recipients_returns_400() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [], "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-10"));
}

#[tokio::test]
#[serial]
async fn send_message_too_many_recipients_returns_400() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;

    let recipients: Vec<String> = (0..11).map(|i| format!("dd_fake_{i}")).collect();

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": recipients, "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("1-10"));
}

#[tokio::test]
#[serial]
async fn send_message_duplicate_recipients_returns_400() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    let resp = server
        .post("/messages/send")
        .add_header(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {}", sender.api_key).parse::<axum::http::HeaderValue>().unwrap(),
        )
        .json(&json!({"to": [recipient.agent_id, recipient.agent_id], "body": "Hello!"}))
        .await;

    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);

    let body: ErrorResponse = resp.json();
    assert!(body.error.contains("duplicate"));
}
