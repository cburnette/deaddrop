use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub description: String,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentSummary>,
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
