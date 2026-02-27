use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use sqlx::PgPool;
use tracing::{error, warn};

use crate::models::api_key::hash_api_key;
use crate::repo::api_key::{find_by_hash, ResolvedApiKey};

/// Redis cache key prefix for resolved API keys.
const CACHE_PREFIX: &str = "gw:api_key:";

/// Cache TTL in seconds (5 minutes).
const CACHE_TTL_SECS: i64 = 300;

/// Middleware that resolves a Bearer API key from the Authorization header.
///
/// Flow:
/// 1. Extract Bearer token from Authorization header
/// 2. SHA-256 hash the raw key
/// 3. Check Redis cache for resolved key data
/// 4. On cache miss, query DB and cache the result
/// 5. Inject `ResolvedApiKey` into request extensions
pub async fn api_key_resolver(
    mut req: Request<Body>,
    next: Next,
) -> Result<Response<Body>, StatusCode> {
    // Extract Bearer token
    let raw_key = extract_bearer_token(&req).ok_or_else(|| {
        warn!("Missing or invalid Authorization header");
        StatusCode::UNAUTHORIZED
    })?;

    let key_hash = hash_api_key(raw_key);

    // Try Redis cache first
    let redis_client = req.extensions().get::<redis::Client>().cloned();
    if let Some(ref client) = redis_client {
        if let Some(resolved) = get_cached_key(client, &key_hash).await {
            req.extensions_mut().insert(resolved);
            return Ok(next.run(req).await);
        }
    }

    // Cache miss — query DB
    let pool = req.extensions().get::<PgPool>().cloned().ok_or_else(|| {
        error!("PgPool not found in request extensions");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let resolved = find_by_hash(&pool, &key_hash)
        .await
        .map_err(|e| {
            error!("Database error resolving API key: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            warn!("API key not found or inactive");
            StatusCode::UNAUTHORIZED
        })?;

    // Cache the resolved key in Redis (best-effort)
    if let Some(ref client) = redis_client {
        cache_resolved_key(client, &key_hash, &resolved).await;
    }

    req.extensions_mut().insert(resolved);
    Ok(next.run(req).await)
}

/// Extract Bearer token from Authorization header.
fn extract_bearer_token(req: &Request<Body>) -> Option<&str> {
    let header = req.headers().get("authorization")?.to_str().ok()?;
    let token = header.strip_prefix("Bearer ")?;
    if token.is_empty() {
        return None;
    }
    Some(token)
}

/// Try to get a cached resolved key from Redis.
async fn get_cached_key(client: &redis::Client, key_hash: &str) -> Option<ResolvedApiKey> {
    let cache_key = format!("{}{}", CACHE_PREFIX, key_hash);
    match crate::redis_ops::get_value(client, &cache_key).await {
        Ok(Some(json_str)) => match serde_json::from_str::<ResolvedApiKey>(&json_str) {
            Ok(resolved) => Some(resolved),
            Err(e) => {
                warn!("Failed to deserialize cached API key: {}", e);
                None
            }
        },
        Ok(None) => None,
        Err(e) => {
            warn!("Redis error on API key cache lookup: {}", e);
            None
        }
    }
}

/// Cache a resolved key in Redis with TTL.
async fn cache_resolved_key(client: &redis::Client, key_hash: &str, resolved: &ResolvedApiKey) {
    let cache_key = format!("{}{}", CACHE_PREFIX, key_hash);
    let json_str = match serde_json::to_string(resolved) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to serialize resolved API key for cache: {}", e);
            return;
        }
    };
    if let Err(e) = crate::redis_ops::set_ex(client, &cache_key, &json_str, CACHE_TTL_SECS).await {
        warn!("Redis error caching resolved API key: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;

    fn build_request_with_auth(auth_value: &str) -> Request<Body> {
        Request::builder()
            .header(AUTHORIZATION, auth_value)
            .body(Body::empty())
            .unwrap()
    }

    fn build_request_no_auth() -> Request<Body> {
        Request::builder().body(Body::empty()).unwrap()
    }

    #[test]
    fn test_extract_bearer_token_valid() {
        let req = build_request_with_auth("Bearer bnk_live_abc123");
        let token = extract_bearer_token(&req);
        assert_eq!(token, Some("bnk_live_abc123"));
    }

    #[test]
    fn test_extract_bearer_token_missing_header() {
        let req = build_request_no_auth();
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_wrong_scheme() {
        let req = build_request_with_auth("Basic abc123");
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_empty_token() {
        let req = build_request_with_auth("Bearer ");
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_extract_bearer_token_no_space() {
        let req = build_request_with_auth("Bearerabc123");
        let token = extract_bearer_token(&req);
        assert!(token.is_none());
    }

    #[test]
    fn test_cache_key_format() {
        let hash = "abcdef1234567890";
        let cache_key = format!("{}{}", CACHE_PREFIX, hash);
        assert_eq!(cache_key, "gw:api_key:abcdef1234567890");
    }
}
