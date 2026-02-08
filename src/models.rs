use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub active: bool,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct AgentsListResponse {
    pub agents: Vec<AgentSummary>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
