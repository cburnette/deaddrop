use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::Commands;

use crate::auth;
use crate::models::{AgentProfileResponse, ErrorResponse, UpdateAgentRequest};

async fn set_active(
    client: &redis::Client,
    headers: &HeaderMap,
    active: bool,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "missing Authorization header".into(),
                }),
            )
        })?;

    let agent_id = auth::verify_bearer(client, auth_header).map_err(|status| {
        (
            status,
            Json(ErrorResponse {
                error: "invalid or missing auth token".into(),
            }),
        )
    })?;

    let mut con = client.get_connection().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis unavailable: {e}"),
            }),
        )
    })?;

    let active_str = if active { "true" } else { "false" };
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let _: () = redis::pipe()
        .hset(format!("agent:{agent_id}"), "active", active_str)
        .hset(format!("agent:{agent_id}"), "updated_at", &now)
        .exec(&mut con)
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn deactivate(
    State(client): State<redis::Client>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    set_active(&client, &headers, false).await
}

pub async fn activate(
    State(client): State<redis::Client>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    set_active(&client, &headers, true).await
}

pub async fn profile(
    State(client): State<redis::Client>,
    headers: HeaderMap,
) -> Result<Json<AgentProfileResponse>, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "missing Authorization header".into(),
                }),
            )
        })?;

    let agent_id = auth::verify_bearer(&client, auth_header).map_err(|status| {
        (
            status,
            Json(ErrorResponse {
                error: "invalid or missing auth token".into(),
            }),
        )
    })?;

    let mut con = client.get_connection().map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis unavailable: {e}"),
            }),
        )
    })?;

    let fields: std::collections::HashMap<String, String> =
        con.hgetall(format!("agent:{agent_id}")).map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    Ok(Json(AgentProfileResponse {
        agent_id,
        name: fields.get("name").cloned().unwrap_or_default(),
        description: fields.get("description").cloned().unwrap_or_default(),
        active: fields.get("active").map(|v| v == "true").unwrap_or(false),
        created_at: fields.get("created_at").cloned().unwrap_or_default(),
        updated_at: fields.get("updated_at").cloned(),
    }))
}

pub async fn update(
    State(client): State<redis::Client>,
    headers: HeaderMap,
    Json(payload): Json<UpdateAgentRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "missing Authorization header".into(),
                }),
            )
        })?;

    let agent_id = auth::verify_bearer(&client, auth_header).map_err(|status| {
        (
            status,
            Json(ErrorResponse {
                error: "invalid or missing auth token".into(),
            }),
        )
    })?;

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

    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    redis::pipe()
        .hset(format!("agent:{agent_id}"), "description", description)
        .hset(format!("agent:{agent_id}"), "updated_at", &now)
        .exec(&mut con)
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}
