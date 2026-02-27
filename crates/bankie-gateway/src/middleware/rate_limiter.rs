use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use tracing::{error, warn};

use crate::redis_ops;
use crate::repo::api_key::ResolvedApiKey;

/// Default burst capacity (max tokens in bucket).
const DEFAULT_BURST: i64 = 100;

/// Default sustained rate (requests per minute).
const DEFAULT_SUSTAINED_PER_MIN: i64 = 1000;

/// Redis key prefix for rate limiting.
const RATE_LIMIT_PREFIX: &str = "gw:rate:";

/// Rate limiter middleware using Redis token bucket algorithm.
///
/// Requires `ResolvedApiKey` in extensions (from api_key_resolver).
/// Sets response headers: X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset.
pub async fn rate_limiter(req: Request<Body>, next: Next) -> Result<Response<Body>, StatusCode> {
    let resolved = req
        .extensions()
        .get::<ResolvedApiKey>()
        .cloned()
        .ok_or_else(|| {
            error!("ResolvedApiKey not found in extensions — api_key_resolver must run first");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let redis_client = req.extensions().get::<redis::Client>().cloned();

    let rate_key = format!("{}{}", RATE_LIMIT_PREFIX, resolved.api_key_id);
    let now_secs = chrono::Utc::now().timestamp();

    // If Redis is unavailable, fail open (allow the request)
    let result = match redis_client {
        Some(ref client) => {
            match redis_ops::rate_limit_check(
                client,
                &rate_key,
                DEFAULT_BURST,
                DEFAULT_SUSTAINED_PER_MIN,
                now_secs,
            )
            .await
            {
                Ok(r) => Some(r),
                Err(e) => {
                    warn!("Rate limiter Redis error, failing open: {}", e);
                    None
                }
            }
        }
        None => {
            warn!("Redis client not available for rate limiting, failing open");
            None
        }
    };

    match result {
        Some(rl) if !rl.allowed => {
            warn!(
                api_key_id = %resolved.api_key_id,
                "Rate limit exceeded"
            );
            let mut response = Response::new(Body::from(
                serde_json::json!({
                    "code": 429,
                    "message": "Rate limit exceeded"
                })
                .to_string(),
            ));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            set_rate_limit_headers(&mut response, DEFAULT_BURST, rl.remaining, rl.reset_at);
            Ok(response)
        }
        Some(rl) => {
            let mut response = next.run(req).await;
            set_rate_limit_headers(&mut response, DEFAULT_BURST, rl.remaining, rl.reset_at);
            Ok(response)
        }
        // Fail open: no rate limit info available
        None => Ok(next.run(req).await),
    }
}

/// Set rate limit response headers.
fn set_rate_limit_headers(
    response: &mut Response<Body>,
    limit: i64,
    remaining: i64,
    reset_at: i64,
) {
    let headers = response.headers_mut();
    headers.insert("X-RateLimit-Limit", limit.into());
    headers.insert("X-RateLimit-Remaining", remaining.into());
    headers.insert("X-RateLimit-Reset", reset_at.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_key_format() {
        let api_key_id = uuid::Uuid::new_v4();
        let key = format!("{}{}", RATE_LIMIT_PREFIX, api_key_id);
        assert!(key.starts_with("gw:rate:"));
        assert!(key.len() > RATE_LIMIT_PREFIX.len());
    }

    #[test]
    fn test_set_rate_limit_headers() {
        let mut response = Response::new(Body::empty());
        set_rate_limit_headers(&mut response, 100, 42, 1700000000);

        assert_eq!(response.headers().get("X-RateLimit-Limit").unwrap(), "100");
        assert_eq!(
            response.headers().get("X-RateLimit-Remaining").unwrap(),
            "42"
        );
        assert_eq!(
            response.headers().get("X-RateLimit-Reset").unwrap(),
            "1700000000"
        );
    }

    #[test]
    fn test_default_burst_and_sustained() {
        assert_eq!(DEFAULT_BURST, 100);
        assert_eq!(DEFAULT_SUSTAINED_PER_MIN, 1000);
    }
}
