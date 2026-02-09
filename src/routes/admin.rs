use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use redis::{Commands, Value};

use crate::models::{
    AdminStatsResponse, AgentStats, ErrorResponse, InboxEntry, InboxStats, MessageStats,
    RedisStats, SearchIndexStats,
};

fn parse_info_field(info: &str, field: &str) -> String {
    info.lines()
        .find(|line| line.starts_with(&format!("{field}:")))
        .and_then(|line| line.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

pub async fn stats(
    State(client): State<redis::Client>,
    headers: HeaderMap,
) -> Result<Json<AdminStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Verify admin secret
    let admin_secret = std::env::var("DEADDROP_ADMIN_SECRET").map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "admin secret not configured".into(),
            }),
        )
    })?;

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "missing Authorization header".into(),
                }),
            )
        })?;

    if auth_header != admin_secret {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid admin secret".into(),
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

    // Total agents
    let total_agents: u64 = con.zcard("agents:created").unwrap_or(0);

    // Active agents via RediSearch
    let active_agents: u64 = redis::cmd("FT.SEARCH")
        .arg("idx:agents")
        .arg("@active:{true}")
        .arg("LIMIT")
        .arg(0)
        .arg(0)
        .query::<Vec<Value>>(&mut con)
        .ok()
        .and_then(|v| match v.first() {
            Some(Value::Int(n)) => Some(*n as u64),
            _ => None,
        })
        .unwrap_or(0);

    // Total stored messages
    let message_keys: Vec<String> = redis::cmd("KEYS")
        .arg("message:*")
        .query(&mut con)
        .unwrap_or_default();
    let total_messages = message_keys.len() as u64;

    // Inbox stats
    let inbox_keys: Vec<String> = redis::cmd("KEYS")
        .arg("inbox:*")
        .query(&mut con)
        .unwrap_or_default();

    let mut total_queued: u64 = 0;
    let mut entries: Vec<InboxEntry> = Vec::new();

    for key in &inbox_keys {
        let count: u64 = con.llen(key).unwrap_or(0);
        if count > 0 {
            total_queued += count;
            let agent_id = key.strip_prefix("inbox:").unwrap_or(key).to_string();
            entries.push(InboxEntry { agent_id, count });
        }
    }

    // Sort by count descending, take top 10
    entries.sort_by(|a, b| b.count.cmp(&a.count));
    entries.truncate(10);

    // Search index doc count — use FT.SEARCH with wildcard, LIMIT 0 0 returns just the count
    let num_docs: u64 = redis::cmd("FT.SEARCH")
        .arg("idx:agents")
        .arg("*")
        .arg("LIMIT")
        .arg(0)
        .arg(0)
        .query::<Vec<Value>>(&mut con)
        .ok()
        .and_then(|v| match v.first() {
            Some(Value::Int(n)) => Some(*n as u64),
            _ => None,
        })
        .unwrap_or(0);

    // Redis INFO
    let memory_info: String = redis::cmd("INFO")
        .arg("memory")
        .query(&mut con)
        .unwrap_or_default();
    let clients_info: String = redis::cmd("INFO")
        .arg("clients")
        .query(&mut con)
        .unwrap_or_default();
    let server_info: String = redis::cmd("INFO")
        .arg("server")
        .query(&mut con)
        .unwrap_or_default();

    let used_memory_human = parse_info_field(&memory_info, "used_memory_human");
    let connected_clients: u64 = parse_info_field(&clients_info, "connected_clients")
        .parse()
        .unwrap_or(0);
    let uptime_seconds: u64 = parse_info_field(&server_info, "uptime_in_seconds")
        .parse()
        .unwrap_or(0);

    Ok(Json(AdminStatsResponse {
        agents: AgentStats {
            total: total_agents,
            active: active_agents,
        },
        messages: MessageStats {
            total_stored: total_messages,
        },
        inboxes: InboxStats {
            total_queued,
            busiest: entries,
        },
        search_index: SearchIndexStats { num_docs },
        redis: RedisStats {
            used_memory_human,
            connected_clients,
            uptime_seconds,
        },
    }))
}
