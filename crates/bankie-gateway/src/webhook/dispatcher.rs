use tracing::{error, info, warn};

use crate::state::PortalState;

/// Single cycle of the fan-out job.
///
/// Polls `portal.webhook_events` for unprocessed events, finds matching
/// endpoints by tenant_id + event_type, creates delivery records, and
/// marks events as processed.
pub async fn run_fanout_cycle(state: &PortalState) {
    // 1. Poll unprocessed events (limit 100)
    let events = match state.webhook_repo.list_unprocessed_events(100).await {
        Ok(events) => events,
        Err(e) => {
            error!("Fan-out: failed to query unprocessed events: {}", e);
            return;
        }
    };

    if events.is_empty() {
        return;
    }

    info!("Fan-out: processing {} webhook events", events.len());

    let mut processed_ids = Vec::new();

    for event in &events {
        // 2. Find matching endpoints by tenant_id + event_type
        let endpoints = match state
            .webhook_repo
            .find_active_endpoints_for_tenant(event.tenant_id, event.event_type.clone())
            .await
        {
            Ok(eps) => eps,
            Err(e) => {
                warn!(
                    "Fan-out: failed to find endpoints for event {}: {}",
                    event.id, e
                );
                continue;
            }
        };

        if endpoints.is_empty() {
            // No endpoints subscribed — still mark as processed
            processed_ids.push(event.id);
            continue;
        }

        // 3. Create delivery records for each matching endpoint
        let mut all_deliveries_ok = true;
        for ep in &endpoints {
            let delivery_id = uuid::Uuid::new_v4();
            match state
                .webhook_repo
                .create_delivery(
                    delivery_id,
                    ep.id,
                    event.event_type.clone(),
                    event.source_id.clone(),
                    event.payload.clone(),
                )
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    // Dedup constraint violation is expected — not an error
                    let msg = e.to_string();
                    if msg.contains("conflict") || msg.contains("duplicate") {
                        info!(
                            "Fan-out: skipping duplicate delivery for endpoint {} event {}",
                            ep.id, event.source_id
                        );
                    } else {
                        warn!(
                            "Fan-out: failed to create delivery for endpoint {} event {}: {}",
                            ep.id, event.id, e
                        );
                        all_deliveries_ok = false;
                    }
                }
            }
        }

        if all_deliveries_ok {
            processed_ids.push(event.id);
        }
    }

    // 4. Mark events as processed
    if !processed_ids.is_empty() {
        if let Err(e) = state
            .webhook_repo
            .mark_events_processed(processed_ids.clone())
            .await
        {
            error!(
                "Fan-out: failed to mark {} events as processed: {}",
                processed_ids.len(),
                e
            );
        } else {
            info!(
                "Fan-out: marked {} events as processed",
                processed_ids.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::models::webhook::{EndpointStatus, WebhookDelivery, WebhookEndpoint, WebhookEvent};
    use crate::repo::api_key::MockApiKeyRepository;
    use crate::repo::dashboard::MockDashboardRepository;
    use crate::repo::member::MockMemberRepository;
    use crate::repo::org::MockOrgRepository;
    use crate::repo::webhook::MockWebhookRepository;
    use crate::repo::RepoError;

    fn make_state(webhook_repo: MockWebhookRepository) -> PortalState {
        PortalState {
            org_repo: Arc::new(MockOrgRepository::new()),
            member_repo: Arc::new(MockMemberRepository::new()),
            api_key_repo: Arc::new(MockApiKeyRepository::new()),
            dashboard_repo: Arc::new(MockDashboardRepository::new()),
            webhook_repo: Arc::new(webhook_repo),
            jwt_secret: "test-secret".to_string(),
            redis_client: None,
        }
    }

    fn test_event(id: i64, tenant_id: i32, event_type: &str) -> WebhookEvent {
        WebhookEvent {
            id,
            tenant_id,
            event_type: event_type.to_string(),
            aggregate_type: "bank_account".to_string(),
            aggregate_id: "agg-1".to_string(),
            source_id: format!("bank_account_agg-1_{}", id),
            payload: serde_json::json!({"test": true}),
            processed: false,
            created_at: chrono::Utc::now(),
        }
    }

    fn test_endpoint(org_id: uuid::Uuid, event_types: Vec<&str>) -> WebhookEndpoint {
        WebhookEndpoint {
            id: uuid::Uuid::new_v4(),
            org_id,
            url: "https://example.com/webhook".to_string(),
            signing_secret: "whsec_test123".to_string(),
            event_types: event_types.into_iter().map(String::from).collect(),
            description: None,
            status: EndpointStatus::Active,
            failure_count: 0,
            disabled_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_fanout_no_events() {
        let mut mock = MockWebhookRepository::new();
        mock.expect_list_unprocessed_events()
            .returning(|_| Ok(vec![]));

        let state = make_state(mock);
        run_fanout_cycle(&state).await;
        // Should return early without error
    }

    #[tokio::test]
    async fn test_fanout_no_matching_endpoints() {
        let mut mock = MockWebhookRepository::new();
        let event = test_event(1, 1, "account.opened");

        mock.expect_list_unprocessed_events()
            .returning(move |_| Ok(vec![event.clone()]));

        mock.expect_find_active_endpoints_for_tenant()
            .returning(|_, _| Ok(vec![]));

        mock.expect_mark_events_processed().returning(|ids| {
            assert_eq!(ids.len(), 1);
            Ok(())
        });

        let state = make_state(mock);
        run_fanout_cycle(&state).await;
    }

    #[tokio::test]
    async fn test_fanout_creates_deliveries() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();
        let event = test_event(1, 1, "account.opened");
        let ep = test_endpoint(org_id, vec!["account.opened"]);

        mock.expect_list_unprocessed_events()
            .returning(move |_| Ok(vec![event.clone()]));

        let ep_clone = ep.clone();
        mock.expect_find_active_endpoints_for_tenant()
            .returning(move |_, _| Ok(vec![ep_clone.clone()]));

        mock.expect_create_delivery()
            .returning(|id, ep_id, evt_type, src_id, payload| {
                Ok(WebhookDelivery {
                    id,
                    endpoint_id: ep_id,
                    event_type: evt_type,
                    event_source_id: src_id,
                    payload,
                    http_status: None,
                    attempt_number: 1,
                    status: crate::models::webhook::DeliveryStatus::Pending,
                    response_body: None,
                    latency_ms: None,
                    next_retry_at: None,
                    created_at: chrono::Utc::now(),
                })
            });

        mock.expect_mark_events_processed().returning(|ids| {
            assert_eq!(ids.len(), 1);
            Ok(())
        });

        let state = make_state(mock);
        run_fanout_cycle(&state).await;
    }

    #[tokio::test]
    async fn test_fanout_handles_dedup_conflict() {
        let mut mock = MockWebhookRepository::new();
        let org_id = uuid::Uuid::new_v4();
        let event = test_event(1, 1, "account.opened");
        let ep = test_endpoint(org_id, vec!["account.opened"]);

        mock.expect_list_unprocessed_events()
            .returning(move |_| Ok(vec![event.clone()]));

        let ep_clone = ep.clone();
        mock.expect_find_active_endpoints_for_tenant()
            .returning(move |_, _| Ok(vec![ep_clone.clone()]));

        // Simulate duplicate conflict
        mock.expect_create_delivery()
            .returning(|_, _, _, _, _| Err(RepoError::Conflict("duplicate".to_string())));

        mock.expect_mark_events_processed().returning(|ids| {
            assert_eq!(ids.len(), 1);
            Ok(())
        });

        let state = make_state(mock);
        run_fanout_cycle(&state).await;
    }

    #[tokio::test]
    async fn test_fanout_query_failure() {
        let mut mock = MockWebhookRepository::new();
        mock.expect_list_unprocessed_events()
            .returning(|_| Err(RepoError::Database("connection lost".to_string())));

        let state = make_state(mock);
        run_fanout_cycle(&state).await;
        // Should not panic
    }
}
