use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::Commands;
use uuid::Uuid;

use axum::extract::Query;
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

    // Rate limit: 12 messages per minute (1 every 5 seconds on average)
    {
        let mut con = client.get_connection().map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis unavailable: {e}"),
                }),
            )
        })?;
        let minute = chrono::Utc::now().format("%Y%m%d%H%M").to_string();
        let rl_key = format!("rl:{sender_id}:{minute}");
        let count: i64 = redis::pipe()
            .atomic()
            .incr(&rl_key, 1)
            .expire(&rl_key, 120)
            .query::<(i64, bool)>(&mut con)
            .map(|(c, _)| c)
            .map_err(|e| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        error: format!("Redis error: {e}"),
                    }),
                )
            })?;
        if count > 12 {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(ErrorResponse {
                    error: "rate limit exceeded: max 12 messages per minute".into(),
                }),
            ));
        }
    }

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
    let mut fields: Vec<(&str, &str)> = vec![
        ("from", sender_id.as_str()),
        ("to", &to_json),
        ("body", body),
        ("timestamp", &timestamp),
    ];
    if let Some(ref reply_to) = payload.reply_to {
        fields.push(("reply_to", reply_to.as_str()));
    }
    pipe.hset_multiple(&message_key, &fields)
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

    // Read all current inbox entries and trim them off.
    // New messages RPUSH'd during this window land after our snapshot and survive the LTRIM.
    let all_ids: Vec<String> = con.lrange(&inbox_key, 0, -1).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis error: {e}"),
            }),
        )
    })?;

    let count = all_ids.len() as isize;
    if count > 0 {
        let _: () = con.ltrim(&inbox_key, count, -1).map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;
    }

    // Pipeline EXISTS checks to filter out expired/evicted messages
    if all_ids.is_empty() {
        return Ok(Json(InboxResponse {
            messages: Vec::new(),
            remaining: 0,
        }));
    }

    let mut exists_pipe = redis::pipe();
    for msg_id in &all_ids {
        exists_pipe.exists(format!("message:{msg_id}"));
    }
    let exists_results: Vec<bool> = exists_pipe.query(&mut con).map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("Redis error: {e}"),
            }),
        )
    })?;

    // Collect only valid (existing) message IDs
    let valid_ids: Vec<&String> = all_ids
        .iter()
        .zip(exists_results.iter())
        .filter(|(_, exists)| **exists)
        .map(|(id, _)| id)
        .collect();

    // Take the first N for the response, push the rest back to the front
    let take_n = take as usize;
    let (to_return, to_keep) = if valid_ids.len() <= take_n {
        (valid_ids.as_slice(), &[] as &[&String])
    } else {
        valid_ids.split_at(take_n)
    };

    // Push remaining valid IDs back to the front of the inbox (preserves FIFO order)
    if !to_keep.is_empty() {
        // LPUSH pushes in reverse order, so we reverse to maintain original order
        let mut push_pipe = redis::pipe();
        for msg_id in to_keep.iter().rev() {
            push_pipe.lpush(&inbox_key, msg_id.as_str());
        }
        push_pipe.exec(&mut con).map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: format!("Redis error: {e}"),
                }),
            )
        })?;
    }

    let remaining = to_keep.len() as u64;

    // Fetch full message details for the batch we're returning
    let mut messages = Vec::new();
    for msg_id in to_return {
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
            message_id: msg_id.to_string(),
            from: fields.get("from").cloned().unwrap_or_default(),
            to,
            body: fields.get("body").cloned().unwrap_or_default(),
            timestamp: fields.get("timestamp").cloned().unwrap_or_default(),
            reply_to: fields.get("reply_to").cloned(),
        });
    }

    Ok(Json(InboxResponse {
        messages,
        remaining,
    }))
}
