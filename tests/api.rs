use axum_test::TestServer;
use deaddrop::models::{
    AdminStatsResponse, ErrorResponse, InboxResponse, RegisterResponse, SearchResponse,
    SendMessageResponse,
};
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

// --- Poll tests ---

fn auth_header(api_key: &str) -> (axum::http::header::HeaderName, axum::http::HeaderValue) {
    (
        axum::http::header::AUTHORIZATION,
        format!("Bearer {api_key}")
            .parse::<axum::http::HeaderValue>()
            .unwrap(),
    )
}

#[tokio::test]
#[serial]
async fn poll_returns_sent_message_with_full_details() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    let sent: SendMessageResponse = server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({"to": [recipient.agent_id], "body": "Hello there!"}))
        .await
        .json();

    let resp = server
        .get("/messages")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await;

    resp.assert_status_ok();

    let body: InboxResponse = resp.json();
    assert_eq!(body.messages.len(), 1);
    assert_eq!(body.messages[0].message_id, sent.message_id);
    assert_eq!(body.messages[0].from, sender.agent_id);
    assert_eq!(body.messages[0].to, vec![recipient.agent_id.clone()]);
    assert_eq!(body.messages[0].body, "Hello there!");
    assert_eq!(body.remaining, 0);
}

#[tokio::test]
#[serial]
async fn poll_consumes_messages() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({"to": [recipient.agent_id], "body": "Message 1"}))
        .await;

    // First poll gets the message
    let body: InboxResponse = server
        .get("/messages")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();
    assert_eq!(body.messages.len(), 1);

    // Second poll returns empty
    let body: InboxResponse = server
        .get("/messages")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();
    assert!(body.messages.is_empty());
    assert_eq!(body.remaining, 0);
}

#[tokio::test]
#[serial]
async fn poll_empty_inbox_returns_empty() {
    let server = test_server();
    let agent = register_agent(&server, "lonely-bot", "No messages").await;

    let resp = server
        .get("/messages")
        .add_header(auth_header(&agent.api_key).0, auth_header(&agent.api_key).1)
        .await;

    resp.assert_status_ok();

    let body: InboxResponse = resp.json();
    assert!(body.messages.is_empty());
    assert_eq!(body.remaining, 0);
}

#[tokio::test]
#[serial]
async fn poll_without_auth_returns_401() {
    let server = test_server();

    let resp = server.get("/messages").await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn poll_returns_messages_in_fifo_order() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    for i in 1..=3 {
        server
            .post("/messages/send")
            .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
            .json(&json!({"to": [recipient.agent_id], "body": format!("Message {i}")}))
            .await;
    }

    let body: InboxResponse = server
        .get("/messages?take=10")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();

    assert_eq!(body.messages.len(), 3);
    assert_eq!(body.messages[0].body, "Message 1");
    assert_eq!(body.messages[1].body, "Message 2");
    assert_eq!(body.messages[2].body, "Message 3");
}

#[tokio::test]
#[serial]
async fn poll_only_returns_messages_for_recipient() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recv1 = register_agent(&server, "recv-one", "First").await;
    let recv2 = register_agent(&server, "recv-two", "Second").await;

    server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({"to": [recv1.agent_id], "body": "For recv1 only"}))
        .await;

    // recv2 should have no messages
    let body: InboxResponse = server
        .get("/messages")
        .add_header(auth_header(&recv2.api_key).0, auth_header(&recv2.api_key).1)
        .await
        .json();
    assert!(body.messages.is_empty());

    // sender should have no messages
    let body: InboxResponse = server
        .get("/messages")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .await
        .json();
    assert!(body.messages.is_empty());
}

#[tokio::test]
#[serial]
async fn poll_take_limits_messages_returned() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    for i in 1..=5 {
        server
            .post("/messages/send")
            .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
            .json(&json!({"to": [recipient.agent_id], "body": format!("Message {i}")}))
            .await;
    }

    let body: InboxResponse = server
        .get("/messages?take=2")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();

    assert_eq!(body.messages.len(), 2);
    assert_eq!(body.messages[0].body, "Message 1");
    assert_eq!(body.messages[1].body, "Message 2");
}

#[tokio::test]
#[serial]
async fn poll_remaining_reflects_unconsumed() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    for i in 1..=5 {
        server
            .post("/messages/send")
            .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
            .json(&json!({"to": [recipient.agent_id], "body": format!("Message {i}")}))
            .await;
    }

    // Take 2, should have 3 remaining
    let body: InboxResponse = server
        .get("/messages?take=2")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();

    assert_eq!(body.messages.len(), 2);
    assert_eq!(body.remaining, 3);

    // Take 2 more, should have 1 remaining
    let body: InboxResponse = server
        .get("/messages?take=2")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();

    assert_eq!(body.messages.len(), 2);
    assert_eq!(body.remaining, 1);

    // Take last one
    let body: InboxResponse = server
        .get("/messages?take=2")
        .add_header(auth_header(&recipient.api_key).0, auth_header(&recipient.api_key).1)
        .await
        .json();

    assert_eq!(body.messages.len(), 1);
    assert_eq!(body.remaining, 0);
}

#[tokio::test]
#[serial]
async fn send_multi_recipient_delivers_to_each_inbox() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recv1 = register_agent(&server, "recv-one", "First").await;
    let recv2 = register_agent(&server, "recv-two", "Second").await;
    let recv3 = register_agent(&server, "recv-three", "Third").await;

    let sent: SendMessageResponse = server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({
            "to": [recv1.agent_id, recv2.agent_id, recv3.agent_id],
            "body": "Hello everyone!"
        }))
        .await
        .json();

    // Each recipient should see the same message
    for (recv, recv_key) in [
        (&recv1, &recv1.api_key),
        (&recv2, &recv2.api_key),
        (&recv3, &recv3.api_key),
    ] {
        let inbox: InboxResponse = server
            .get("/messages")
            .add_header(auth_header(recv_key).0, auth_header(recv_key).1)
            .await
            .json();

        assert_eq!(inbox.messages.len(), 1, "expected 1 message for {}", recv.name);
        assert_eq!(inbox.messages[0].message_id, sent.message_id);
        assert_eq!(inbox.messages[0].from, sender.agent_id);
        assert_eq!(inbox.messages[0].body, "Hello everyone!");
        assert_eq!(inbox.messages[0].to.len(), 3);
    }

    // Sender should have no messages
    let inbox: InboxResponse = server
        .get("/messages")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .await
        .json();
    assert!(inbox.messages.is_empty());
}

#[tokio::test]
#[serial]
async fn send_message_sets_ttl_on_message() {
    let server = test_server();
    let sender = register_agent(&server, "sender-bot", "Sends").await;
    let recipient = register_agent(&server, "recv-bot", "Receives").await;

    let sent: SendMessageResponse = server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({"to": [recipient.agent_id], "body": "Ephemeral message"}))
        .await
        .json();

    // Verify TTL is set on the message key (should be ~7 days = 604800 seconds)
    let client = redis::Client::open("redis://127.0.0.1/0").unwrap();
    let mut con = client.get_connection().unwrap();
    let ttl: i64 = redis::cmd("TTL")
        .arg(format!("message:{}", sent.message_id))
        .query(&mut con)
        .unwrap();

    // TTL should be close to 604800 (7 days), allow some margin
    assert!(ttl > 604700, "TTL was {ttl}, expected ~604800");
}

// --- Admin stats tests ---

const TEST_ADMIN_SECRET: &str = "test-admin-secret-12345";

fn set_admin_secret() {
    unsafe { std::env::set_var("DEADDROP_ADMIN_SECRET", TEST_ADMIN_SECRET) };
}

fn admin_auth() -> (axum::http::header::HeaderName, axum::http::HeaderValue) {
    (
        axum::http::header::AUTHORIZATION,
        format!("Bearer {TEST_ADMIN_SECRET}")
            .parse::<axum::http::HeaderValue>()
            .unwrap(),
    )
}

#[tokio::test]
#[serial]
async fn admin_stats_returns_200_with_valid_secret() {
    set_admin_secret();
    let server = test_server();

    let resp = server
        .get("/admin/stats")
        .add_header(admin_auth().0, admin_auth().1)
        .await;

    resp.assert_status_ok();

    let body: AdminStatsResponse = resp.json();
    assert_eq!(body.agents.total, 0);
    assert_eq!(body.agents.active, 0);
    assert_eq!(body.messages.total_stored, 0);
    assert_eq!(body.inboxes.total_queued, 0);
    assert!(body.inboxes.busiest.is_empty());
    assert!(body.redis.uptime_seconds > 0);
}

#[tokio::test]
#[serial]
async fn admin_stats_without_auth_returns_401() {
    set_admin_secret();
    let server = test_server();

    let resp = server.get("/admin/stats").await;
    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn admin_stats_wrong_secret_returns_401() {
    set_admin_secret();
    let server = test_server();

    let resp = server
        .get("/admin/stats")
        .add_header(
            axum::http::header::AUTHORIZATION,
            "Bearer wrong-secret".parse::<axum::http::HeaderValue>().unwrap(),
        )
        .await;

    resp.assert_status(axum::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial]
async fn admin_stats_reflects_data() {
    set_admin_secret();
    let server = test_server();

    // Register 2 agents
    let sender = register_agent(&server, "sender-bot", "Sends messages").await;
    let recipient = register_agent(&server, "recv-bot", "Receives messages").await;

    // Send a message
    server
        .post("/messages/send")
        .add_header(auth_header(&sender.api_key).0, auth_header(&sender.api_key).1)
        .json(&json!({"to": [recipient.agent_id], "body": "Hello!"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    let resp = server
        .get("/admin/stats")
        .add_header(admin_auth().0, admin_auth().1)
        .await;

    resp.assert_status_ok();

    let body: AdminStatsResponse = resp.json();
    assert_eq!(body.agents.total, 2);
    assert_eq!(body.agents.active, 2);
    assert_eq!(body.messages.total_stored, 1);
    assert_eq!(body.inboxes.total_queued, 1);
    assert_eq!(body.inboxes.busiest.len(), 1);
    assert_eq!(body.inboxes.busiest[0].agent_id, recipient.agent_id);
    assert_eq!(body.inboxes.busiest[0].count, 1);
    assert_eq!(body.search_index.num_docs, 2);
}

#[tokio::test]
#[serial]
async fn admin_stats_exercises_full_system() {
    set_admin_secret();
    let server = test_server();

    // Register 5 agents
    let a1 = register_agent(&server, "alpha-bot", "Alpha agent for coordination").await;
    let a2 = register_agent(&server, "beta-bot", "Beta agent for analysis").await;
    let a3 = register_agent(&server, "gamma-bot", "Gamma agent for reporting").await;
    let a4 = register_agent(&server, "delta-bot", "Delta agent for monitoring").await;
    let a5 = register_agent(&server, "epsilon-bot", "Epsilon agent for cleanup").await;

    // a1 sends 3 messages to a2
    for i in 1..=3 {
        server
            .post("/messages/send")
            .add_header(auth_header(&a1.api_key).0, auth_header(&a1.api_key).1)
            .json(&json!({"to": [a2.agent_id], "body": format!("Message {i} to beta")}))
            .await
            .assert_status(axum::http::StatusCode::CREATED);
    }

    // a1 sends a multi-recipient message to a3, a4, a5
    server
        .post("/messages/send")
        .add_header(auth_header(&a1.api_key).0, auth_header(&a1.api_key).1)
        .json(&json!({"to": [a3.agent_id, a4.agent_id, a5.agent_id], "body": "Broadcast!"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // a3 sends 2 messages to a2
    for i in 1..=2 {
        server
            .post("/messages/send")
            .add_header(auth_header(&a3.api_key).0, auth_header(&a3.api_key).1)
            .json(&json!({"to": [a2.agent_id], "body": format!("From gamma {i}")}))
            .await
            .assert_status(axum::http::StatusCode::CREATED);
    }

    // a2 polls 2 of their 5 messages (3 from a1 + 2 from a3)
    let inbox: InboxResponse = server
        .get("/messages?take=2")
        .add_header(auth_header(&a2.api_key).0, auth_header(&a2.api_key).1)
        .await
        .json();
    assert_eq!(inbox.messages.len(), 2);
    assert_eq!(inbox.remaining, 3);

    // Now check admin stats
    let body: AdminStatsResponse = server
        .get("/admin/stats")
        .add_header(admin_auth().0, admin_auth().1)
        .await
        .json();

    // 5 agents, all active
    assert_eq!(body.agents.total, 5);
    assert_eq!(body.agents.active, 5);

    // 6 messages stored (3 + 1 broadcast + 2)
    assert_eq!(body.messages.total_stored, 6);

    // Inbox state after a2 polled 2:
    //   a2: 3 remaining (5 - 2 polled)
    //   a3: 1 (from broadcast)
    //   a4: 1 (from broadcast)
    //   a5: 1 (from broadcast)
    //   a1: 0 (never received anything)
    // total queued: 6
    assert_eq!(body.inboxes.total_queued, 6);

    // a2 should be the busiest with 3 remaining
    assert_eq!(body.inboxes.busiest[0].agent_id, a2.agent_id);
    assert_eq!(body.inboxes.busiest[0].count, 3);

    // 4 agents have messages in their inbox (a2, a3, a4, a5)
    assert_eq!(body.inboxes.busiest.len(), 4);

    // Search index has all 5 agents
    assert_eq!(body.search_index.num_docs, 5);

    // Redis stats should be populated
    assert!(!body.redis.used_memory_human.is_empty());
    assert!(body.redis.connected_clients > 0);
    assert!(body.redis.uptime_seconds > 0);
}
