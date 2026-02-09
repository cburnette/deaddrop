use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::Commands;
use uuid::Uuid;

use crate::auth;
use crate::models::{ErrorResponse, SendMessageRequest, SendMessageResponse};

pub async fn send(
    State(client): State<redis::Client>,
    headers: HeaderMap,
    Json(payload): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Authenticate sender
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

    let sender_id = auth::verify_bearer(&client, auth_header).map_err(|status| {
        (
            status,
            Json(ErrorResponse {
                error: "invalid or missing auth token".into(),
            }),
        )
    })?;

    // Validate recipients
    if payload.to.is_empty() || payload.to.len() > 10 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "to must contain 1-10 recipients".into(),
            }),
        ));
    }

    // Check for duplicates
    let mut seen = std::collections::HashSet::new();
    for recipient in &payload.to {
        if !seen.insert(recipient.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "duplicate recipients are not allowed".into(),
                }),
            ));
        }
    }

    // Check for self-send
    if payload.to.contains(&sender_id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "cannot send a message to yourself".into(),
            }),
        ));
    }

    // Validate body
    let body = payload.body.trim();
    if body.is_empty() || body.len() > 32768 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "body must be 1-32768 characters".into(),
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

    // Verify each recipient exists and is active
    for recipient in &payload.to {
        let active: Option<String> =
            con.hget(format!("agent:{recipient}"), "active")
                .map_err(|e| {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(ErrorResponse {
                            error: format!("Redis error: {e}"),
                        }),
                    )
                })?;

        if active.as_deref() != Some("true") {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("recipient '{recipient}' not found or inactive"),
                }),
            ));
        }
    }

    // Generate message ID and timestamp
    let message_id = format!("msg_{}", Uuid::new_v4());
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to_json = serde_json::to_string(&payload.to).unwrap();

    // Store message and deliver to inboxes
    let message_key = format!("message:{message_id}");
    let mut pipe = redis::pipe();
    pipe.hset_multiple(
        &message_key,
        &[
            ("from", sender_id.as_str()),
            ("to", &to_json),
            ("body", body),
            ("timestamp", &timestamp),
        ],
    );
    for recipient in &payload.to {
        pipe.rpush(format!("inbox:{recipient}"), &message_id);
    }
    pipe.exec(&mut con).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis error: {e}"),
            }),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(SendMessageResponse {
            message_id,
            from: sender_id,
            to: payload.to,
            timestamp,
        }),
    ))
}
