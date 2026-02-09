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
pub struct AgentProfileResponse {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub description: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub to: Vec<String>,
    pub body: String,
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SendMessageResponse {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub timestamp: String,
}

#[derive(Deserialize)]
pub struct PollParams {
    pub take: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct InboxMessage {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub body: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct InboxResponse {
    pub messages: Vec<InboxMessage>,
    pub remaining: u64,
}

#[derive(Serialize, Deserialize)]
pub struct AgentStats {
    pub total: u64,
    pub active: u64,
}

#[derive(Serialize, Deserialize)]
pub struct MessageStats {
    pub total_stored: u64,
}

#[derive(Serialize, Deserialize)]
pub struct InboxEntry {
    pub agent_id: String,
    pub count: u64,
}

#[derive(Serialize, Deserialize)]
pub struct InboxStats {
    pub total_queued: u64,
    pub busiest: Vec<InboxEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct SearchIndexStats {
    pub num_docs: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RedisStats {
    pub used_memory_human: String,
    pub connected_clients: u64,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize)]
pub struct AdminStatsResponse {
    pub agents: AgentStats,
    pub messages: MessageStats,
    pub inboxes: InboxStats,
    pub search_index: SearchIndexStats,
    pub redis: RedisStats,
}
