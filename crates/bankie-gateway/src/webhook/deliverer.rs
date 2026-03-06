use chrono::{Duration, Utc};
use rand::Rng;
use tracing::{error, info, warn};

use crate::models::webhook::PendingDelivery;
use crate::state::PortalState;
use crate::webhook::signing::sign_payload;
use crate::webhook::ssrf::validate_url_safe;

/// Maximum number of delivery attempts before dead-lettering.
const MAX_ATTEMPTS: i32 = 7;

/// Number of consecutive endpoint failures before disabling the endpoint.
const CIRCUIT_BREAKER_THRESHOLD: i32 = 5;

/// HTTP timeout for webhook delivery in seconds.
const DELIVERY_TIMEOUT_SECS: u64 = 30;

/// Max concurrent deliveries per cycle.
const MAX_CONCURRENT_DELIVERIES: usize = 10;

/// Backoff schedule in seconds: 30s, 2m, 15m, 1h, 4h, 12h, 24h
const BACKOFF_SCHEDULE: [i64; 7] = [
    30,    // attempt 1 → retry after 30s
    120,   // attempt 2 → retry after 2m
    900,   // attempt 3 → retry after 15m
    3600,  // attempt 4 → retry after 1h
    14400, // attempt 5 → retry after 4h
    43200, // attempt 6 → retry after 12h
    86400, // attempt 7 → retry after 24h (final, then dead letter)
];

/// Calculate the next retry time with ±10% jitter.
pub fn calculate_next_retry(attempt: i32) -> Option<chrono::DateTime<Utc>> {
    let idx = (attempt - 1) as usize;
    if idx >= BACKOFF_SCHEDULE.len() {
        return None; // Should be dead-lettered
    }

    let base_secs = BACKOFF_SCHEDULE[idx];
    let jitter_range = base_secs / 10; // ±10%
    let jitter = if jitter_range > 0 {
        rand::thread_rng().gen_range(-jitter_range..=jitter_range)
    } else {
        0
    };
    let delay_secs = (base_secs + jitter).max(1);

    Some(Utc::now() + Duration::seconds(delay_secs))
}

/// Single cycle of the delivery job.
///
/// Polls pending deliveries, sends HTTP requests with signed payloads,
/// and updates delivery status based on response.
pub async fn run_delivery_cycle(state: &PortalState) {
    // 1. Poll pending deliveries
    let pending = match state.webhook_repo.list_pending_deliveries(50).await {
        Ok(p) => p,
        Err(e) => {
            error!("Delivery: failed to query pending deliveries: {}", e);
            return;
        }
    };

    if pending.is_empty() {
        return;
    }

    info!("Delivery: processing {} pending deliveries", pending.len());

    // 2. Build HTTP client with timeout
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DELIVERY_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("Delivery: failed to build HTTP client: {}", e);
            return;
        }
    };

    // 3. Process deliveries concurrently (up to MAX_CONCURRENT_DELIVERIES)
    let mut join_set = tokio::task::JoinSet::new();

    for delivery in pending {
        if join_set.len() >= MAX_CONCURRENT_DELIVERIES {
            // Wait for one to complete before spawning more
            if let Some(Err(e)) = join_set.join_next().await {
                error!("Delivery task panicked: {}", e);
            }
        }

        let client = client.clone();
        let webhook_repo = state.webhook_repo.clone();

        join_set.spawn(async move {
            deliver_single(&client, &*webhook_repo, delivery).await;
        });
    }

    // Drain remaining tasks
    while join_set.join_next().await.is_some() {}
}

/// Deliver a single webhook to its endpoint.
async fn deliver_single(
    client: &reqwest::Client,
    webhook_repo: &dyn crate::repo::webhook::WebhookRepository,
    pending: PendingDelivery,
) {
    let delivery = &pending.delivery;
    let delivery_id = delivery.id;

    // Defense-in-depth: SSRF check at delivery time (DNS may have changed since creation)
    // Skip in local/docker environments to allow development testing
    let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
    if env != "local" && env != "docker" {
        if let Err(reason) = validate_url_safe(&pending.endpoint_url) {
            warn!(
                "Delivery {}: SSRF blocked — {} (endpoint {})",
                delivery_id, reason, delivery.endpoint_id
            );
            // Treat as permanent failure — dead-letter immediately
            if let Err(e) = webhook_repo.move_to_dead_letter(delivery_id).await {
                error!(
                    "Delivery {}: failed to dead-letter after SSRF block: {}",
                    delivery_id, e
                );
            }
            return;
        }
    }

    let body = serde_json::to_string(&delivery.payload).unwrap_or_default();
    let timestamp = Utc::now().timestamp();

    // Sign the payload
    let signature = sign_payload(&pending.signing_secret, timestamp, &body);

    let start = std::time::Instant::now();

    // Send HTTP request
    let result = client
        .post(&pending.endpoint_url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "Bankie-Webhooks/1.0")
        .header("X-Bankie-Signature", &signature)
        .header("X-Bankie-Event-Id", &delivery.event_source_id)
        .header("X-Bankie-Webhook-Id", delivery_id.to_string())
        .body(body)
        .send()
        .await;

    let latency_ms = start.elapsed().as_millis() as i32;

    match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as i32;

            if resp.status().is_success() {
                // Success — mark delivery and reset endpoint failure count
                if let Err(e) = webhook_repo
                    .update_delivery_success(delivery_id, status, latency_ms)
                    .await
                {
                    error!("Delivery {}: failed to mark success: {}", delivery_id, e);
                }

                if let Err(e) = webhook_repo.reset_failure_count(delivery.endpoint_id).await {
                    warn!(
                        "Delivery {}: failed to reset failure count: {}",
                        delivery_id, e
                    );
                }

                info!(
                    "Delivery {}: success (HTTP {}, {}ms)",
                    delivery_id, status, latency_ms
                );
            } else {
                // Non-2xx — handle failure
                // Truncate response body to 4KB to prevent OOM from malicious endpoints
                let response_body = resp
                    .bytes()
                    .await
                    .ok()
                    .map(|b| String::from_utf8_lossy(&b[..b.len().min(4096)]).to_string());
                handle_delivery_failure(webhook_repo, &pending, status, latency_ms, response_body)
                    .await;
            }
        }
        Err(e) => {
            // Timeout or connection error
            let response_body = Some(e.to_string());
            handle_delivery_failure(webhook_repo, &pending, 0, latency_ms, response_body).await;
        }
    }
}

/// Handle a delivery failure — retry, dead-letter, or circuit-break.
async fn handle_delivery_failure(
    webhook_repo: &dyn crate::repo::webhook::WebhookRepository,
    pending: &PendingDelivery,
    http_status: i32,
    latency_ms: i32,
    response_body: Option<String>,
) {
    let delivery = &pending.delivery;
    let delivery_id = delivery.id;
    let attempt = delivery.attempt_number;

    if attempt >= MAX_ATTEMPTS {
        // Dead letter — maximum attempts exhausted
        warn!(
            "Delivery {}: dead-lettered after {} attempts",
            delivery_id, attempt
        );
        if let Err(e) = webhook_repo.move_to_dead_letter(delivery_id).await {
            error!(
                "Delivery {}: failed to mark dead letter: {}",
                delivery_id, e
            );
        }
    } else {
        // Schedule retry with exponential backoff
        let next_retry = calculate_next_retry(attempt);
        let opt_status = if http_status == 0 {
            None
        } else {
            Some(http_status)
        };
        if let Err(e) = webhook_repo
            .update_delivery_failure(
                delivery_id,
                opt_status,
                Some(latency_ms),
                response_body.clone(),
                next_retry,
            )
            .await
        {
            error!("Delivery {}: failed to mark failure: {}", delivery_id, e);
        }

        info!(
            "Delivery {}: failed (HTTP {}, attempt {}/{}), next retry at {:?}",
            delivery_id, http_status, attempt, MAX_ATTEMPTS, next_retry
        );
    }

    // Circuit breaker: increment failure count, disable if threshold reached
    match webhook_repo
        .increment_failure_count(delivery.endpoint_id)
        .await
    {
        Ok(count) if count >= CIRCUIT_BREAKER_THRESHOLD => {
            warn!(
                "Circuit breaker: disabling endpoint {} after {} failures",
                delivery.endpoint_id, count
            );
            if let Err(e) = webhook_repo.disable_endpoint(delivery.endpoint_id).await {
                error!(
                    "Circuit breaker: failed to disable endpoint {}: {}",
                    delivery.endpoint_id, e
                );
            }
        }
        Ok(_) => {}
        Err(e) => {
            warn!(
                "Delivery {}: failed to increment failure count: {}",
                delivery_id, e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_schedule_values() {
        assert_eq!(BACKOFF_SCHEDULE[0], 30); // 30s
        assert_eq!(BACKOFF_SCHEDULE[1], 120); // 2m
        assert_eq!(BACKOFF_SCHEDULE[2], 900); // 15m
        assert_eq!(BACKOFF_SCHEDULE[3], 3600); // 1h
        assert_eq!(BACKOFF_SCHEDULE[4], 14400); // 4h
        assert_eq!(BACKOFF_SCHEDULE[5], 43200); // 12h
        assert_eq!(BACKOFF_SCHEDULE[6], 86400); // 24h
    }

    #[test]
    fn test_calculate_next_retry_attempt_1() {
        let result = calculate_next_retry(1);
        assert!(result.is_some());
        let retry_at = result.unwrap();
        let diff = (retry_at - Utc::now()).num_seconds();
        // 30s ± 10% = 27..33
        assert!((26..=34).contains(&diff), "diff was {diff}");
    }

    #[test]
    fn test_calculate_next_retry_attempt_2() {
        let result = calculate_next_retry(2);
        assert!(result.is_some());
        let retry_at = result.unwrap();
        let diff = (retry_at - Utc::now()).num_seconds();
        // 120s ± 10% = 108..132
        assert!((107..=133).contains(&diff), "diff was {diff}");
    }

    #[test]
    fn test_calculate_next_retry_attempt_7() {
        let result = calculate_next_retry(7);
        assert!(result.is_some());
        let retry_at = result.unwrap();
        let diff = (retry_at - Utc::now()).num_seconds();
        // 86400s ± 10% = 77760..95040
        assert!((77759..=95041).contains(&diff), "diff was {diff}");
    }

    #[test]
    fn test_calculate_next_retry_beyond_max() {
        let result = calculate_next_retry(8);
        assert!(result.is_none());
    }

    #[test]
    fn test_constants() {
        assert_eq!(MAX_ATTEMPTS, 7);
        assert_eq!(CIRCUIT_BREAKER_THRESHOLD, 5);
        assert_eq!(DELIVERY_TIMEOUT_SECS, 30);
        assert_eq!(MAX_CONCURRENT_DELIVERIES, 10);
    }

    #[test]
    fn test_backoff_total_duration() {
        // Total retry window: 30s + 2m + 15m + 1h + 4h + 12h + 24h ≈ ~41h
        let total: i64 = BACKOFF_SCHEDULE.iter().sum();
        assert_eq!(total, 148_650); // 41h 17m 30s
    }

    #[tokio::test]
    async fn test_delivery_cycle_no_pending() {
        use crate::repo::api_key::MockApiKeyRepository;
        use crate::repo::dashboard::MockDashboardRepository;
        use crate::repo::member::MockMemberRepository;
        use crate::repo::org::MockOrgRepository;
        use crate::repo::webhook::MockWebhookRepository;
        use std::sync::Arc;

        let mut mock = MockWebhookRepository::new();
        mock.expect_list_pending_deliveries()
            .returning(|_| Ok(vec![]));

        let state = PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            webhook_repo: Arc::new(mock),
            db_pools: crate::config::DbPools::test_dummy(),
            jwt_secret: "test".to_string(),
            redis_client: None,
        };

        run_delivery_cycle(&state).await;
        // Should return early without error
    }

    #[tokio::test]
    async fn test_delivery_cycle_query_failure() {
        use crate::repo::api_key::MockApiKeyRepository;
        use crate::repo::dashboard::MockDashboardRepository;
        use crate::repo::member::MockMemberRepository;
        use crate::repo::org::MockOrgRepository;
        use crate::repo::webhook::MockWebhookRepository;
        use crate::repo::RepoError;
        use std::sync::Arc;

        let mut mock = MockWebhookRepository::new();
        mock.expect_list_pending_deliveries()
            .returning(|_| Err(RepoError::Database("connection lost".to_string())));

        let state = PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            webhook_repo: Arc::new(mock),
            db_pools: crate::config::DbPools::test_dummy(),
            jwt_secret: "test".to_string(),
            redis_client: None,
        };

        run_delivery_cycle(&state).await;
        // Should not panic
    }
}
