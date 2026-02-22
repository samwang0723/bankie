use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

use crate::repository::redis::{get_value, set_nx_ex};

const IDEMPOTENCY_HEADER: &str = "Idempotency-Key";
const IDEMPOTENCY_TTL: i64 = 86400; // 24 hours

/// Extracts the Idempotency-Key header value if present.
pub fn get_idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get(IDEMPOTENCY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Idempotency middleware for POST endpoints.
/// If `Idempotency-Key` header is present:
///   - Check Redis for existing key (scoped by tenant_id)
///   - If found: return cached "already processed" response
///   - If not found: set key in Redis with NX + 24h TTL, proceed
pub async fn idempotency_check(req: Request, next: Next) -> Result<Response<Body>, StatusCode> {
    let key = get_idempotency_key(req.headers());

    if let Some(key) = key {
        // Try to get Redis client from extensions
        let cache = req.extensions().get::<Arc<redis::Client>>().cloned();
        // Get tenant_id from auth middleware (inserted by authorize)
        let tenant_id = req.extensions().get::<i32>().copied().unwrap_or(0);

        if let Some(cache) = cache {
            let redis_key = format!("idempotency:{}:{}", tenant_id, key);

            // Check if key already exists
            match get_value(&cache, &redis_key).await {
                Ok(Some(_)) => {
                    // Already processed — return 409 Conflict
                    return Ok((
                        StatusCode::CONFLICT,
                        Json(json!({
                            "code": 409,
                            "message": "Duplicate request: this Idempotency-Key has already been processed"
                        })),
                    )
                        .into_response());
                }
                Ok(None) => {
                    // Set the key with NX + TTL
                    match set_nx_ex(&cache, &redis_key, "1", IDEMPOTENCY_TTL).await {
                        Ok(true) => {
                            // Key set successfully, proceed
                        }
                        Ok(false) => {
                            // Race condition: another request set it first
                            return Ok((
                                StatusCode::CONFLICT,
                                Json(json!({
                                    "code": 409,
                                    "message": "Duplicate request: this Idempotency-Key has already been processed"
                                })),
                            )
                                .into_response());
                        }
                        Err(e) => {
                            warn!("Redis error during idempotency check: {:?}", e);
                            // Fail open: proceed without idempotency protection
                        }
                    }
                }
                Err(e) => {
                    warn!("Redis error during idempotency lookup: {:?}", e);
                    // Fail open: proceed without idempotency protection
                }
            }
        }
    }

    Ok(next.run(req).await)
}
