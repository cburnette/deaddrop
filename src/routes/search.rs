use axum::{extract::State, http::StatusCode, Json};
use redis::Value;

use crate::models::{ErrorResponse, SearchRequest, SearchResponse, SearchResult};

/// Strip RediSearch special characters to prevent query injection.
fn sanitize_phrase(phrase: &str) -> String {
    let special = [
        '@', '!', '{', '}', '(', ')', '|', '-', '=', '>', '[', ']', ':', ';', '*', '~', '\\',
        '"', '\'', '/', '<', '.', ',', '$', '%', '^', '&', '#', '+', '?',
    ];
    phrase
        .chars()
        .filter(|c| !special.contains(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a RediSearch query: OR phrases together, filter by active.
fn build_query(phrases: &[String]) -> Option<String> {
    let sanitized: Vec<String> = phrases
        .iter()
        .map(|p| sanitize_phrase(p))
        .filter(|s| !s.is_empty())
        .map(|s| format!("\"{s}\""))
        .collect();

    if sanitized.is_empty() {
        return None;
    }

    let phrase_query = sanitized.join(" | ");
    Some(format!("@active:{{true}} ({phrase_query})"))
}

/// Parse FT.SEARCH RESP2 array response into SearchResults.
fn parse_search_results(value: Value) -> Result<Vec<SearchResult>, String> {
    let items = match value {
        Value::Array(items) => items,
        _ => return Err("unexpected FT.SEARCH response type".into()),
    };

    // First element is the total count
    if items.is_empty() {
        return Err("empty FT.SEARCH response".into());
    }

    let mut results = Vec::new();
    // After count, pairs of (key, [field, value, field, value, ...])
    let mut i = 1;
    while i + 1 < items.len() {
        let key = match &items[i] {
            Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
            _ => {
                i += 2;
                continue;
            }
        };

        // Extract agent_id from key "agent:dd_..."
        let agent_id = key.strip_prefix("agent:").unwrap_or(&key).to_string();

        let fields = match &items[i + 1] {
            Value::Array(f) => f,
            _ => {
                i += 2;
                continue;
            }
        };

        let mut name = String::new();
        let mut description = String::new();

        let mut j = 0;
        while j + 1 < fields.len() {
            let field_name = match &fields[j] {
                Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                _ => {
                    j += 2;
                    continue;
                }
            };
            let field_val = match &fields[j + 1] {
                Value::BulkString(b) => String::from_utf8_lossy(b).to_string(),
                _ => String::new(),
            };

            match field_name.as_str() {
                "name" => name = field_val,
                "description" => description = field_val,
                _ => {}
            }

            j += 2;
        }

        results.push(SearchResult {
            agent_id,
            name,
            description,
        });

        i += 2;
    }

    Ok(results)
}

pub async fn search(
    State(client): State<redis::Client>,
    Json(payload): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate phrases
    if payload.phrases.is_empty() || payload.phrases.len() > 10 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "phrases must contain 1-10 items".into(),
            }),
        ));
    }

    for phrase in &payload.phrases {
        if phrase.is_empty() || phrase.len() > 256 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "each phrase must be 1-256 characters".into(),
                }),
            ));
        }
    }

    let query = build_query(&payload.phrases).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "phrases contain no searchable content after sanitization".into(),
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

    let result: Value = redis::cmd("FT.SEARCH")
        .arg("idx:agents")
        .arg(&query)
        .arg("LIMIT")
        .arg(0)
        .arg(50)
        .query(&mut con)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("search error: {e}"),
                }),
            )
        })?;

    let results = parse_search_results(result).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("parse error: {e}"),
            }),
        )
    })?;

    Ok(Json(SearchResponse { results }))
}
