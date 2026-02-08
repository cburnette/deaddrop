use axum::{extract::State, http::StatusCode, Json};
use redis::Commands;
use uuid::Uuid;

use crate::models::{ErrorResponse, RegisterRequest, RegisterResponse};

pub async fn register(
    State(client): State<redis::Client>,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Validate name
    let name = payload.name.trim();
    if name.len() < 3 || name.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "name must be 3-128 characters".into(),
            }),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "name must contain only alphanumeric characters, hyphens, or underscores"
                    .into(),
            }),
        ));
    }

    // Validate description
    let description = payload.description.trim();
    if description.is_empty() || description.len() > 1024 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "description must be 1-1024 characters".into(),
            }),
        ));
    }

    let mut con = client.get_connection().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis unavailable: {e}"),
            }),
        )
    })?;

    // Generate agent ID
    let agent_id = format!("dd_{}", Uuid::new_v4());
    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    // Atomic name reservation with SETNX
    let name_key = format!("agent:name:{name}");
    let was_set: bool = con.set_nx(&name_key, &agent_id).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis error: {e}"),
            }),
        )
    })?;

    if !was_set {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: format!("name '{name}' is already taken"),
            }),
        ));
    }

    // Store agent hash
    let agent_key = format!("agent:{agent_id}");
    redis::pipe()
        .hset_multiple(
            &agent_key,
            &[
                ("name", name),
                ("description", description),
                ("active", "true"),
                ("created_at", &created_at),
            ],
        )
        .zadd("agents:created", &agent_id, chrono::Utc::now().timestamp() as f64)
        .exec(&mut con)
        .map_err(|e| {
            // Best-effort cleanup of name reservation on failure
            let _: Result<(), _> = con.del(&name_key);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterResponse {
            agent_id,
            name: name.to_string(),
            description: description.to_string(),
            active: true,
            created_at,
        }),
    ))
}
