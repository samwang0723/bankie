use std::sync::Arc;

use tokio::time::{interval, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::models::api_key::KeyStatus;
use crate::models::dashboard::NewAuditLog;
use crate::redis_ops;
use crate::state::PortalState;
use crate::webhook::{deliverer, dispatcher};

/// Redis lock key for grace period expiry job.
const GRACE_EXPIRY_LOCK_KEY: &str = "gw:lock:grace_expiry";

/// Redis lock key for webhook fan-out job.
const FANOUT_LOCK_KEY: &str = "gw:lock:webhook_fanout";

/// Redis lock key for webhook delivery job.
const DELIVERY_LOCK_KEY: &str = "gw:lock:webhook_deliver";

/// Lock timeout in seconds.
const LOCK_TIMEOUT_SECS: i64 = 120;

/// Redis cache key prefix for resolved API keys (must match api_key_resolver).
const API_KEY_CACHE_PREFIX: &str = "gw:api_key:";

/// Spawn the grace period expiry background job.
///
/// Runs every 60 seconds, finds rotated API keys past their grace period,
/// revokes them, deletes their Redis cache, and logs to audit.
pub fn spawn_grace_expiry_job(state: Arc<PortalState>) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            run_grace_expiry_cycle(&state).await;
        }
    });
}

/// Spawn the webhook fan-out background job.
///
/// Runs every 5 seconds, polls webhook_events for unprocessed events,
/// fans them out to matching endpoints as delivery records.
pub fn spawn_webhook_fanout_job(state: Arc<PortalState>) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            run_webhook_fanout_cycle(&state).await;
        }
    });
}

/// Single cycle of the fan-out job with Redis lock.
async fn run_webhook_fanout_cycle(state: &PortalState) {
    let redis_client = match &state.redis_client {
        Some(c) => c,
        None => return,
    };

    let lock_id = match acquire_lock(redis_client, FANOUT_LOCK_KEY, 60).await {
        Some(id) => id,
        None => return,
    };

    dispatcher::run_fanout_cycle(state).await;

    release_lock(redis_client, FANOUT_LOCK_KEY, &lock_id).await;
}

/// Spawn the webhook delivery background job.
///
/// Runs every 5 seconds, polls pending deliveries and sends HTTP requests
/// with signed payloads, handling retries and circuit breaking.
pub fn spawn_webhook_delivery_job(state: Arc<PortalState>) {
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            run_webhook_delivery_cycle(&state).await;
        }
    });
}

/// Single cycle of the delivery job with Redis lock.
async fn run_webhook_delivery_cycle(state: &PortalState) {
    let redis_client = match &state.redis_client {
        Some(c) => c,
        None => return,
    };

    let lock_id = match acquire_lock(redis_client, DELIVERY_LOCK_KEY, 60).await {
        Some(id) => id,
        None => return,
    };

    deliverer::run_delivery_cycle(state).await;

    release_lock(redis_client, DELIVERY_LOCK_KEY, &lock_id).await;
}

/// Single cycle of the grace period expiry job.
async fn run_grace_expiry_cycle(state: &PortalState) {
    let redis_client = match &state.redis_client {
        Some(c) => c,
        None => {
            warn!("Redis not available, skipping grace expiry cycle");
            return;
        }
    };

    // Acquire distributed lock
    let lock_id = match acquire_lock(redis_client, GRACE_EXPIRY_LOCK_KEY, LOCK_TIMEOUT_SECS).await {
        Some(id) => id,
        None => return, // Another instance holds the lock
    };

    // Find expired rotated keys
    let expired_keys = match state.api_key_repo.list_expired_rotated().await {
        Ok(keys) => keys,
        Err(e) => {
            error!("Failed to query expired rotated keys: {}", e);
            release_lock(redis_client, GRACE_EXPIRY_LOCK_KEY, &lock_id).await;
            return;
        }
    };

    if expired_keys.is_empty() {
        release_lock(redis_client, GRACE_EXPIRY_LOCK_KEY, &lock_id).await;
        return;
    }

    info!(
        "Grace expiry job: found {} expired rotated keys",
        expired_keys.len()
    );

    for key in &expired_keys {
        // Revoke the key
        match state
            .api_key_repo
            .update_status(key.id, KeyStatus::Revoked, None)
            .await
        {
            Ok(Some(_)) => {
                info!("Revoked expired rotated key: {}", key.id);

                // Delete Redis cache for this key's hash
                let cache_key = format!("{}{}", API_KEY_CACHE_PREFIX, key.key_hash);
                if let Err(e) = redis_ops::del_key(redis_client, &cache_key).await {
                    warn!("Failed to delete cache for key {}: {}", key.id, e);
                }

                // Audit log (best-effort)
                let audit = NewAuditLog {
                    org_id: key.org_id,
                    actor_id: Uuid::nil(),
                    action: "api_key.grace_expired".to_string(),
                    resource_type: "api_key".to_string(),
                    resource_id: Some(key.id.to_string()),
                    changes: Some(serde_json::json!({
                        "status": {"from": "rotated", "to": "revoked"},
                        "reason": "grace_period_expired"
                    })),
                    client_ip: None,
                };
                if let Err(e) = state.dashboard_repo.insert_audit_log(audit).await {
                    warn!("Failed to audit grace expiry for key {}: {}", key.id, e);
                }
            }
            Ok(None) => {
                warn!("Key {} not found during grace expiry revocation", key.id);
            }
            Err(e) => {
                error!("Failed to revoke expired key {}: {}", key.id, e);
            }
        }
    }

    release_lock(redis_client, GRACE_EXPIRY_LOCK_KEY, &lock_id).await;
}

/// Acquire a Redis distributed lock using SET NX with expiry.
async fn acquire_lock(client: &redis::Client, key: &str, timeout_secs: i64) -> Option<String> {
    let lock_value = Uuid::new_v4().to_string();
    match redis_ops::set_nx_ex(client, key, &lock_value, timeout_secs).await {
        Ok(true) => Some(lock_value),
        Ok(false) => None,
        Err(e) => {
            warn!("Failed to acquire lock {}: {}", key, e);
            None
        }
    }
}

/// Release a Redis distributed lock (only if we own it).
async fn release_lock(client: &redis::Client, key: &str, expected_value: &str) {
    match redis_ops::get_value(client, key).await {
        Ok(Some(current)) if current == expected_value => {
            if let Err(e) = redis_ops::del_key(client, key).await {
                warn!("Failed to release lock {}: {}", key, e);
            }
        }
        _ => {} // Lock expired or owned by someone else
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::api_key::ApiKey;
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;
    use chrono::{Duration, Utc};

    fn make_expired_key() -> ApiKey {
        ApiKey {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            tenant_id: 1,
            name: "expired-key".to_string(),
            key_prefix: "bk_live_test1234".to_string(),
            key_hash: "abc123hash".to_string(),
            scopes: vec!["accounts:read".to_string()],
            status: KeyStatus::Rotated,
            grace_expires_at: Some(Utc::now() - Duration::hours(1)),
            created_at: Utc::now() - Duration::days(30),
            updated_at: Utc::now() - Duration::hours(1),
        }
    }

    fn make_state_no_redis(
        api_key_repo: MockApiKeyRepository,
        dashboard_repo: MockDashboardRepository,
    ) -> PortalState {
        PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(api_key_repo),
            dashboard_repo: Arc::new(dashboard_repo),
            webhook_repo: Arc::new(MockWebhookRepository::new()),
            db_pools: crate::config::DbPools::test_dummy(),
            jwt_secret: "test-secret".to_string(),
            redis_client: None,
        }
    }

    #[tokio::test]
    async fn test_grace_expiry_skips_when_no_redis() {
        let state =
            make_state_no_redis(MockApiKeyRepository::new(), MockDashboardRepository::new());
        // Should return early without error when Redis is unavailable
        run_grace_expiry_cycle(&state).await;
    }

    #[test]
    fn test_lock_key_constants() {
        assert_eq!(GRACE_EXPIRY_LOCK_KEY, "gw:lock:grace_expiry");
        assert_eq!(LOCK_TIMEOUT_SECS, 120);
    }

    #[test]
    fn test_api_key_cache_prefix() {
        let hash = "abcdef1234567890";
        let cache_key = format!("{}{}", API_KEY_CACHE_PREFIX, hash);
        assert_eq!(cache_key, "gw:api_key:abcdef1234567890");
    }

    #[test]
    fn test_expired_key_fixture() {
        let key = make_expired_key();
        assert_eq!(key.status, KeyStatus::Rotated);
        assert!(key.grace_expires_at.unwrap() < Utc::now());
    }

    #[test]
    fn test_audit_log_format() {
        let key = make_expired_key();
        let audit = NewAuditLog {
            org_id: key.org_id,
            actor_id: Uuid::nil(),
            action: "api_key.grace_expired".to_string(),
            resource_type: "api_key".to_string(),
            resource_id: Some(key.id.to_string()),
            changes: Some(serde_json::json!({
                "status": {"from": "rotated", "to": "revoked"},
                "reason": "grace_period_expired"
            })),
            client_ip: None,
        };
        assert_eq!(audit.action, "api_key.grace_expired");
        assert_eq!(audit.resource_type, "api_key");
        assert!(audit.resource_id.is_some());
        let changes = audit.changes.unwrap();
        assert_eq!(changes["reason"], "grace_period_expired");
    }
}
