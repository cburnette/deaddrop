use axum::http::StatusCode;
use redis::Commands;
use sha2::{Digest, Sha256};

/// SHA-256 hash a token and return the hex-encoded digest.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

/// Generate a new API key and its hash.
/// Returns `(plaintext_token, hex_hash)`.
pub fn generate_api_key() -> (String, String) {
    let bytes: [u8; 32] = rand::random();
    let token = format!("dd_key_{}", hex::encode(bytes));
    let hash = hash_token(&token);
    (token, hash)
}

/// Verify a Bearer token from the Authorization header.
/// Returns the agent_id on success, or an HTTP status code on failure.
pub fn verify_bearer(client: &redis::Client, header: &str) -> Result<String, StatusCode> {
    let token = header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !token.starts_with("dd_key_") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let hash = hash_token(token);
    let auth_key = format!("auth:{hash}");

    let mut con = client
        .get_connection()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    let agent_id: Option<String> = con.get(&auth_key).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    agent_id.ok_or(StatusCode::UNAUTHORIZED)
}
