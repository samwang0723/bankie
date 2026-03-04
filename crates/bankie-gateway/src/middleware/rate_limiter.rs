use std::num::NonZeroU32;

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use governor::{Quota, RateLimiter};
use tracing::{error, warn};

use crate::redis_ops;
use crate::repo::api_key::ResolvedApiKey;

/// Default burst capacity (max tokens in bucket).
pub const DEFAULT_BURST_CAP: i64 = 100;

/// Default sustained rate (requests per minute).
pub const DEFAULT_SUSTAINED_CAP: i64 = 1000;

/// In-memory fallback burst capacity (per instance, lower than Redis).
const FALLBACK_BURST: u32 = 50;

/// In-memory fallback sustained rate (requests per minute, per instance).
const FALLBACK_SUSTAINED_PER_MIN: u32 = 500;

/// Redis key prefix for rate limiting.
const RATE_LIMIT_PREFIX: &str = "gw:rate:";

/// Redis key prefix for tracking throttled request counts (24h window).
pub const THROTTLED_PREFIX: &str = "gw:throttled:";

/// TTL for throttled counters (24 hours).
const THROTTLED_TTL_SECS: i64 = 86400;

lazy_static::lazy_static! {
    /// In-memory keyed rate limiter using governor, used as fallback when Redis is unavailable.
    static ref FALLBACK_LIMITER: RateLimiter<
        String,
        governor::state::keyed::DashMapStateStore<String>,
        governor::clock::DefaultClock,
    > = {
        let quota = Quota::per_minute(NonZeroU32::new(FALLBACK_SUSTAINED_PER_MIN).unwrap())
            .allow_burst(NonZeroU32::new(FALLBACK_BURST).unwrap());
        RateLimiter::dashmap(quota)
    };
}

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
                DEFAULT_BURST_CAP,
                DEFAULT_SUSTAINED_CAP,
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

            // Track throttled request count in Redis (best-effort, 24h window)
            if let Some(ref client) = redis_client {
                let throttled_key = format!("{}{}", THROTTLED_PREFIX, resolved.api_key_id);
                if let Err(e) =
                    redis_ops::incr_with_expiry(client, &throttled_key, THROTTLED_TTL_SECS).await
                {
                    warn!("Failed to track throttled request: {}", e);
                }
            }

            let mut response = Response::new(Body::from(
                serde_json::json!({
                    "code": 429,
                    "message": "Rate limit exceeded"
                })
                .to_string(),
            ));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            set_rate_limit_headers(&mut response, DEFAULT_BURST_CAP, rl.remaining, rl.reset_at);
            Ok(response)
        }
        Some(rl) => {
            let mut response = next.run(req).await;
            set_rate_limit_headers(&mut response, DEFAULT_BURST_CAP, rl.remaining, rl.reset_at);
            Ok(response)
        }
        // Fallback: in-memory governor rate limiter when Redis is unavailable
        None => {
            let key = resolved.api_key_id.to_string();
            match FALLBACK_LIMITER.check_key(&key) {
                Ok(_) => Ok(next.run(req).await),
                Err(_) => {
                    warn!(
                        api_key_id = %resolved.api_key_id,
                        "Rate limit exceeded (in-memory fallback)"
                    );
                    let mut response = Response::new(Body::from(
                        serde_json::json!({
                            "code": 429,
                            "message": "Rate limit exceeded"
                        })
                        .to_string(),
                    ));
                    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                    Ok(response)
                }
            }
        }
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
        assert_eq!(DEFAULT_BURST_CAP, 100);
        assert_eq!(DEFAULT_SUSTAINED_CAP, 1000);
    }

    #[test]
    fn test_fallback_limiter_constants() {
        assert_eq!(FALLBACK_BURST, 50);
        assert_eq!(FALLBACK_SUSTAINED_PER_MIN, 500);
    }

    #[test]
    fn test_fallback_limiter_allows_request() {
        let key = uuid::Uuid::new_v4().to_string();
        // First request should always be allowed
        assert!(FALLBACK_LIMITER.check_key(&key).is_ok());
    }

    #[test]
    fn test_fallback_limiter_rejects_after_burst() {
        let key = format!("burst-test-{}", uuid::Uuid::new_v4());
        // Exhaust the burst capacity
        for _ in 0..FALLBACK_BURST {
            let _ = FALLBACK_LIMITER.check_key(&key);
        }
        // Next request should be rejected
        assert!(FALLBACK_LIMITER.check_key(&key).is_err());
    }
}
