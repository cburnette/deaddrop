use axum::{extract::State, http::StatusCode, Json};
use redis::Commands;

use crate::models::{ErrorResponse, ListAgentsResponse, SearchResult};

pub async fn list(
    State(client): State<redis::Client>,
) -> Result<Json<ListAgentsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut con = client.get_connection().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis unavailable: {e}"),
            }),
        )
    })?;

    // Get all agent IDs in descending created_at order
    let agent_ids: Vec<String> = redis::cmd("ZREVRANGE")
        .arg("agents:created")
        .arg(0)
        .arg(-1)
        .query(&mut con)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    let mut agents = Vec::new();
    for agent_id in &agent_ids {
        let fields: std::collections::HashMap<String, String> =
            con.hgetall(format!("agent:{agent_id}")).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Redis error: {e}"),
                    }),
                )
            })?;

        if fields.get("active").map(|v| v.as_str()) == Some("true") {
            agents.push(SearchResult {
                agent_id: agent_id.clone(),
                name: fields.get("name").cloned().unwrap_or_default(),
                description: fields.get("description").cloned().unwrap_or_default(),
            });
        }
    }

    Ok(Json(ListAgentsResponse { agents }))
}
