use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::Commands;
use uuid::Uuid;

use axum::extract::Query;
use redis::Value;

use crate::auth;
use crate::models::{
    ErrorResponse, InboxMessage, InboxResponse, PollParams, SendMessageRequest,
    SendMessageResponse,
};

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

    // Store message with 7-day TTL and deliver to inboxes
    let message_key = format!("message:{message_id}");
    let ttl_seconds: i64 = 7 * 24 * 60 * 60;
    let mut pipe = redis::pipe();
    pipe.hset_multiple(
        &message_key,
        &[
            ("from", sender_id.as_str()),
            ("to", &to_json),
            ("body", body),
            ("timestamp", &timestamp),
        ],
    )
    .expire(&message_key, ttl_seconds);
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

pub async fn poll(
    State(client): State<redis::Client>,
    headers: HeaderMap,
    Query(params): Query<PollParams>,
) -> Result<Json<InboxResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Authenticate
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

    // Validate take parameter
    let take = params.take.unwrap_or(1);
    if take < 1 || take > 10 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "take must be 1-10".into(),
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

    let inbox_key = format!("inbox:{agent_id}");

    // Pipeline: read first N, trim those off, get remaining count
    let (message_ids, _, remaining): (Vec<String>, Value, u64) = redis::pipe()
        .lrange(&inbox_key, 0, (take as isize) - 1)
        .ltrim(&inbox_key, take as isize, -1)
        .llen(&inbox_key)
        .query(&mut con)
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;

    // Fetch full message details
    let mut messages = Vec::new();
    for msg_id in &message_ids {
        let fields: std::collections::HashMap<String, String> =
            con.hgetall(format!("message:{msg_id}")).map_err(|e| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: format!("Redis error: {e}"),
                    }),
                )
            })?;

        if fields.is_empty() {
            continue;
        }

        let to: Vec<String> = fields
            .get("to")
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();

        messages.push(InboxMessage {
            message_id: msg_id.clone(),
            from: fields.get("from").cloned().unwrap_or_default(),
            to,
            body: fields.get("body").cloned().unwrap_or_default(),
            timestamp: fields.get("timestamp").cloned().unwrap_or_default(),
        });
    }

    Ok(Json(InboxResponse {
        messages,
        remaining,
    }))
}
