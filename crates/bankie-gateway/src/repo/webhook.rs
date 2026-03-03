use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::RepoError;
use crate::models::webhook::{
    ApiLogEntry, ApiLogFilters, PendingDelivery, WebhookDelivery, WebhookEndpoint, WebhookEvent,
};

#[allow(clippy::too_many_arguments)]
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WebhookRepository: Send + Sync {
    // === Endpoint CRUD ===

    async fn create_endpoint(
        &self,
        id: Uuid,
        org_id: Uuid,
        url: String,
        signing_secret: String,
        event_types: Vec<String>,
        description: Option<String>,
    ) -> Result<WebhookEndpoint, RepoError>;

    async fn find_endpoint_by_id(
        &self,
        id: Uuid,
        org_id: Uuid,
    ) -> Result<Option<WebhookEndpoint>, RepoError>;

    async fn list_endpoints_by_org(&self, org_id: Uuid) -> Result<Vec<WebhookEndpoint>, RepoError>;

    async fn update_endpoint(
        &self,
        id: Uuid,
        org_id: Uuid,
        url: Option<String>,
        event_types: Option<Vec<String>>,
        description: Option<String>,
        status: Option<String>,
    ) -> Result<Option<WebhookEndpoint>, RepoError>;

    async fn delete_endpoint(&self, id: Uuid, org_id: Uuid) -> Result<bool, RepoError>;

    // === Secret Rotation ===

    async fn rotate_signing_secret(
        &self,
        id: Uuid,
        org_id: Uuid,
        new_secret: String,
    ) -> Result<Option<WebhookEndpoint>, RepoError>;

    // === Circuit Breaker (used by delivery job) ===

    async fn increment_failure_count(&self, id: Uuid) -> Result<i32, RepoError>;

    async fn reset_failure_count(&self, id: Uuid) -> Result<(), RepoError>;

    async fn disable_endpoint(&self, id: Uuid) -> Result<(), RepoError>;

    // === Fan-out Queries ===

    async fn find_active_endpoints_for_tenant(
        &self,
        tenant_id: i32,
        event_type: String,
    ) -> Result<Vec<WebhookEndpoint>, RepoError>;

    // === Webhook Events (staging table) ===

    async fn list_unprocessed_events(&self, limit: i64) -> Result<Vec<WebhookEvent>, RepoError>;

    async fn mark_events_processed(&self, ids: Vec<i64>) -> Result<(), RepoError>;

    // === Deliveries ===

    async fn create_delivery(
        &self,
        id: Uuid,
        endpoint_id: Uuid,
        event_type: String,
        event_source_id: String,
        payload: serde_json::Value,
    ) -> Result<WebhookDelivery, RepoError>;

    async fn list_pending_deliveries(&self, limit: i64) -> Result<Vec<PendingDelivery>, RepoError>;

    async fn update_delivery_success(
        &self,
        id: Uuid,
        http_status: i32,
        latency_ms: i32,
    ) -> Result<(), RepoError>;

    async fn update_delivery_failure(
        &self,
        id: Uuid,
        http_status: Option<i32>,
        latency_ms: Option<i32>,
        response_body: Option<String>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), RepoError>;

    async fn move_to_dead_letter(&self, id: Uuid) -> Result<(), RepoError>;

    async fn list_deliveries_by_endpoint(
        &self,
        endpoint_id: Uuid,
        page: i64,
        per_page: i64,
        status_filter: Option<String>,
    ) -> Result<(Vec<WebhookDelivery>, i64), RepoError>;

    async fn find_delivery_by_id(
        &self,
        id: Uuid,
        endpoint_id: Uuid,
    ) -> Result<Option<WebhookDelivery>, RepoError>;

    async fn reset_delivery_for_retry(
        &self,
        id: Uuid,
    ) -> Result<Option<WebhookDelivery>, RepoError>;

    // === API Logs (for viewer) ===

    async fn list_api_logs(
        &self,
        tenant_id: i32,
        filters: ApiLogFilters,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<ApiLogEntry>, i64), RepoError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::webhook::{DeliveryStatus, EndpointStatus};
    use serde_json::json;

    // === Mock-based tests to verify the trait shape compiles with mockall ===

    #[tokio::test]
    async fn test_mock_create_endpoint() {
        let mut mock = MockWebhookRepository::new();
        let endpoint_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();

        mock.expect_create_endpoint()
            .withf(move |id, oid, url, _, _, _| {
                *id == endpoint_id && *oid == org_id && url == "https://example.com"
            })
            .returning(move |id, org_id, url, secret, event_types, description| {
                Ok(WebhookEndpoint {
                    id,
                    org_id,
                    url,
                    signing_secret: secret,
                    event_types,
                    description,
                    status: EndpointStatus::Active,
                    failure_count: 0,
                    disabled_at: None,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            });

        let result = mock
            .create_endpoint(
                endpoint_id,
                org_id,
                "https://example.com".to_string(),
                "whsec_test".to_string(),
                vec!["account.opened".to_string()],
                None,
            )
            .await;

        assert!(result.is_ok());
        let ep = result.unwrap();
        assert_eq!(ep.id, endpoint_id);
        assert_eq!(ep.url, "https://example.com");
        assert_eq!(ep.status, EndpointStatus::Active);
    }

    #[tokio::test]
    async fn test_mock_find_endpoint_by_id() {
        let mut mock = MockWebhookRepository::new();
        let endpoint_id = Uuid::new_v4();
        let org_id = Uuid::new_v4();

        mock.expect_find_endpoint_by_id().returning(|_, _| Ok(None));

        let result = mock.find_endpoint_by_id(endpoint_id, org_id).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_list_endpoints_by_org() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_list_endpoints_by_org()
            .returning(|_| Ok(vec![]));

        let result = mock.list_endpoints_by_org(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mock_delete_endpoint() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_delete_endpoint().returning(|_, _| Ok(true));

        let result = mock.delete_endpoint(Uuid::new_v4(), Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_mock_list_unprocessed_events() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_list_unprocessed_events()
            .returning(|_| Ok(vec![]));

        let result = mock.list_unprocessed_events(100).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mock_mark_events_processed() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_mark_events_processed().returning(|_| Ok(()));

        let result = mock.mark_events_processed(vec![1, 2, 3]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_create_delivery() {
        let mut mock = MockWebhookRepository::new();
        let delivery_id = Uuid::new_v4();
        let endpoint_id = Uuid::new_v4();

        mock.expect_create_delivery()
            .returning(move |id, ep_id, evt, src, payload| {
                Ok(WebhookDelivery {
                    id,
                    endpoint_id: ep_id,
                    event_type: evt,
                    event_source_id: src,
                    payload,
                    http_status: None,
                    attempt_number: 1,
                    status: DeliveryStatus::Pending,
                    response_body: None,
                    latency_ms: None,
                    next_retry_at: Some(Utc::now()),
                    created_at: Utc::now(),
                })
            });

        let result = mock
            .create_delivery(
                delivery_id,
                endpoint_id,
                "account.opened".to_string(),
                "src_123".to_string(),
                json!({"test": true}),
            )
            .await;

        assert!(result.is_ok());
        let delivery = result.unwrap();
        assert_eq!(delivery.id, delivery_id);
        assert_eq!(delivery.status, DeliveryStatus::Pending);
        assert_eq!(delivery.attempt_number, 1);
    }

    #[tokio::test]
    async fn test_mock_update_delivery_success() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_update_delivery_success()
            .returning(|_, _, _| Ok(()));

        let result = mock.update_delivery_success(Uuid::new_v4(), 200, 150).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_update_delivery_failure() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_update_delivery_failure()
            .returning(|_, _, _, _, _| Ok(()));

        let result = mock
            .update_delivery_failure(
                Uuid::new_v4(),
                Some(500),
                Some(30000),
                Some("Internal Server Error".to_string()),
                Some(Utc::now()),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_move_to_dead_letter() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_move_to_dead_letter().returning(|_| Ok(()));

        let result = mock.move_to_dead_letter(Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_circuit_breaker_increment() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_increment_failure_count().returning(|_| Ok(3));

        let result = mock.increment_failure_count(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 3);
    }

    #[tokio::test]
    async fn test_mock_circuit_breaker_reset() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_reset_failure_count().returning(|_| Ok(()));

        let result = mock.reset_failure_count(Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_disable_endpoint() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_disable_endpoint().returning(|_| Ok(()));

        let result = mock.disable_endpoint(Uuid::new_v4()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_find_active_endpoints_for_tenant() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_find_active_endpoints_for_tenant()
            .returning(|_, _| Ok(vec![]));

        let result = mock
            .find_active_endpoints_for_tenant(100, "account.opened".to_string())
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mock_list_deliveries_by_endpoint() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_list_deliveries_by_endpoint()
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let result = mock
            .list_deliveries_by_endpoint(Uuid::new_v4(), 1, 20, None)
            .await;
        assert!(result.is_ok());
        let (deliveries, total) = result.unwrap();
        assert!(deliveries.is_empty());
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_mock_list_api_logs() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_list_api_logs()
            .returning(|_, _, _, _| Ok((vec![], 0)));

        let filters = ApiLogFilters {
            method: Some("POST".to_string()),
            status_code: None,
            path: None,
            from: None,
            to: None,
        };
        let result = mock.list_api_logs(100, filters, 1, 50).await;
        assert!(result.is_ok());
        let (logs, total) = result.unwrap();
        assert!(logs.is_empty());
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_mock_rotate_signing_secret() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_rotate_signing_secret()
            .returning(|_, _, _| Ok(None));

        let result = mock
            .rotate_signing_secret(Uuid::new_v4(), Uuid::new_v4(), "whsec_new".to_string())
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_reset_delivery_for_retry() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_reset_delivery_for_retry()
            .returning(|_| Ok(None));

        let result = mock.reset_delivery_for_retry(Uuid::new_v4()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_repo_error_propagation() {
        let mut mock = MockWebhookRepository::new();

        mock.expect_create_endpoint()
            .returning(|_, _, _, _, _, _| Err(RepoError::Database("connection failed".into())));

        let result = mock
            .create_endpoint(
                Uuid::new_v4(),
                Uuid::new_v4(),
                "https://example.com".to_string(),
                "whsec_test".to_string(),
                vec![],
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RepoError::Database(_)));
    }
}
