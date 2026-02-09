use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::Commands;

use crate::auth;
use crate::models::{ErrorResponse, UpdateAgentRequest};

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
    let _: () = con
        .hset(format!("agent:{agent_id}"), "active", active_str)
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

    let _: () = con
        .hset(format!("agent:{agent_id}"), "description", description)
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
