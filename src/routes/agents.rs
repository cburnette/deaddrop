use axum::{extract::State, http::StatusCode, Json};
use redis::Commands;

use crate::models::{AgentSummary, AgentsListResponse, ErrorResponse};

pub async fn list_agents(
    State(client): State<redis::Client>,
) -> Result<Json<AgentsListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut con = client.get_connection().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis unavailable: {e}"),
            }),
        )
    })?;

    // Get all agent IDs ordered by creation time descending
    let agent_ids: Vec<String> = con.zrevrange("agents:created", 0, -1).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis error: {e}"),
            }),
        )
    })?;

    let mut agents = Vec::new();

    for agent_id in agent_ids {
        let agent_key = format!("agent:{agent_id}");
        let fields: Vec<Option<String>> = redis::pipe()
            .hget(&agent_key, "name")
            .hget(&agent_key, "description")
            .hget(&agent_key, "active")
            .hget(&agent_key, "created_at")
            .query(&mut con)
            .map_err(|e| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: format!("Redis error: {e}"),
                    }),
                )
            })?;

        let active = fields[2].as_deref().unwrap_or("false");
        if active != "true" {
            continue;
        }

        if let (Some(name), Some(description), Some(created_at)) =
            (&fields[0], &fields[1], &fields[3])
        {
            agents.push(AgentSummary {
                agent_id,
                name: name.clone(),
                description: description.clone(),
                created_at: created_at.clone(),
            });
        }
    }

    Ok(Json(AgentsListResponse { agents }))
}
