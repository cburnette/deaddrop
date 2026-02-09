use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub agent_id: String,
    pub api_key: String,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize)]
pub struct SearchRequest {
    pub phrases: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchResult {
    pub agent_id: String,
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub to: Vec<String>,
    pub body: String,
}

#[derive(Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub timestamp: String,
}
