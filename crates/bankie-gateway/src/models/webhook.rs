use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// === Endpoint Status ===

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointStatus {
    Active,
    Disabled,
}

impl EndpointStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EndpointStatus::Active => "active",
            EndpointStatus::Disabled => "disabled",
        }
    }
}

impl std::str::FromStr for EndpointStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(EndpointStatus::Active),
            "disabled" => Ok(EndpointStatus::Disabled),
            _ => Err(format!("unknown endpoint status: {s}")),
        }
    }
}

// === Delivery Status ===

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Success,
    Failed,
    DeadLetter,
}

impl DeliveryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryStatus::Pending => "pending",
            DeliveryStatus::Success => "success",
            DeliveryStatus::Failed => "failed",
            DeliveryStatus::DeadLetter => "dead_letter",
        }
    }
}

impl std::str::FromStr for DeliveryStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(DeliveryStatus::Pending),
            "success" => Ok(DeliveryStatus::Success),
            "failed" => Ok(DeliveryStatus::Failed),
            "dead_letter" => Ok(DeliveryStatus::DeadLetter),
            _ => Err(format!("unknown delivery status: {s}")),
        }
    }
}

// === Allowed Event Types ===

pub const VALID_EVENT_TYPES: &[&str] = &[
    "account.opened",
    "account.approved",
    "account.frozen",
    "account.closed",
    "transaction.initiated",
    "transaction.completed",
    "transaction.failed",
];

/// Validate that all requested event types are in the allowed set.
pub fn validate_event_types(event_types: &[String]) -> Result<(), Vec<String>> {
    let invalid: Vec<String> = event_types
        .iter()
        .filter(|e| !VALID_EVENT_TYPES.contains(&e.as_str()))
        .cloned()
        .collect();
    if invalid.is_empty() {
        Ok(())
    } else {
        Err(invalid)
    }
}

// === Domain Models ===

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WebhookEndpoint {
    pub id: Uuid,
    pub org_id: Uuid,
    pub url: String,
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub signing_secret: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub failure_count: i32,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub event_type: String,
    pub event_source_id: String,
    pub payload: serde_json::Value,
    pub http_status: Option<i32>,
    pub attempt_number: i32,
    pub status: DeliveryStatus,
    pub response_body: Option<String>,
    pub latency_ms: Option<i32>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Staging table row — populated by DB triggers, consumed by fan-out job.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub id: i64,
    pub tenant_id: i32,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub source_id: String,
    pub payload: serde_json::Value,
    pub processed: bool,
    pub created_at: DateTime<Utc>,
}

// === Request DTOs ===

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateEndpointRequest {
    pub url: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateEndpointRequest {
    pub url: Option<String>,
    pub event_types: Option<Vec<String>>,
    pub description: Option<String>,
    pub status: Option<EndpointStatus>,
}

// === Response DTOs ===

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateEndpointResponse {
    pub id: Uuid,
    pub url: String,
    pub signing_secret: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EndpointListItem {
    pub id: Uuid,
    pub url: String,
    pub signing_secret_prefix: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub failure_count: i32,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DeliveryListItem {
    pub id: Uuid,
    pub event_type: String,
    pub event_source_id: String,
    pub status: DeliveryStatus,
    pub http_status: Option<i32>,
    pub attempt_number: i32,
    pub latency_ms: Option<i32>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RotateSecretResponse {
    pub new_signing_secret: String,
    pub grace_expires_at: DateTime<Utc>,
}

/// Joined struct for delivery job (avoids N+1 queries).
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    pub delivery: WebhookDelivery,
    pub endpoint_url: String,
    pub signing_secret: String,
    pub endpoint_failure_count: i32,
}

/// Webhook payload envelope sent to tenant endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub created_at: DateTime<Utc>,
    pub tenant_id: i32,
    pub data: serde_json::Value,
}

// === API Log types (for viewer) ===

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApiLogEntry {
    pub id: i64,
    pub api_key_id: Option<Uuid>,
    pub tenant_id: Option<i32>,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub latency_ms: Option<i32>,
    pub client_ip: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ApiLogFilters {
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub path: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

// === Signing Secret Generation ===

/// Generate a webhook signing secret with `whsec_` prefix.
pub fn generate_signing_secret() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    format!("whsec_{}", hex::encode(bytes))
}

/// Mask a signing secret for display: `whsec_a1b2****`
pub fn mask_signing_secret(secret: &str) -> String {
    if secret.len() > 10 {
        format!("{}****", &secret[..10])
    } else {
        "whsec_****".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- EndpointStatus tests ---

    #[test]
    fn test_endpoint_status_serialization() {
        let json = serde_json::to_string(&EndpointStatus::Active).unwrap();
        assert_eq!(json, "\"active\"");
        let json = serde_json::to_string(&EndpointStatus::Disabled).unwrap();
        assert_eq!(json, "\"disabled\"");
    }

    #[test]
    fn test_endpoint_status_deserialization() {
        let status: EndpointStatus = serde_json::from_str("\"active\"").unwrap();
        assert_eq!(status, EndpointStatus::Active);
        let status: EndpointStatus = serde_json::from_str("\"disabled\"").unwrap();
        assert_eq!(status, EndpointStatus::Disabled);
    }

    #[test]
    fn test_endpoint_status_as_str() {
        assert_eq!(EndpointStatus::Active.as_str(), "active");
        assert_eq!(EndpointStatus::Disabled.as_str(), "disabled");
    }

    #[test]
    fn test_endpoint_status_from_str() {
        assert_eq!(
            "active".parse::<EndpointStatus>(),
            Ok(EndpointStatus::Active)
        );
        assert_eq!(
            "disabled".parse::<EndpointStatus>(),
            Ok(EndpointStatus::Disabled)
        );
        assert!("unknown".parse::<EndpointStatus>().is_err());
    }

    // --- DeliveryStatus tests ---

    #[test]
    fn test_delivery_status_serialization() {
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Success).unwrap(),
            "\"success\""
        );
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&DeliveryStatus::DeadLetter).unwrap(),
            "\"dead_letter\""
        );
    }

    #[test]
    fn test_delivery_status_as_str() {
        assert_eq!(DeliveryStatus::Pending.as_str(), "pending");
        assert_eq!(DeliveryStatus::Success.as_str(), "success");
        assert_eq!(DeliveryStatus::Failed.as_str(), "failed");
        assert_eq!(DeliveryStatus::DeadLetter.as_str(), "dead_letter");
    }

    #[test]
    fn test_delivery_status_from_str() {
        assert_eq!(
            "pending".parse::<DeliveryStatus>(),
            Ok(DeliveryStatus::Pending)
        );
        assert_eq!(
            "success".parse::<DeliveryStatus>(),
            Ok(DeliveryStatus::Success)
        );
        assert_eq!(
            "failed".parse::<DeliveryStatus>(),
            Ok(DeliveryStatus::Failed)
        );
        assert_eq!(
            "dead_letter".parse::<DeliveryStatus>(),
            Ok(DeliveryStatus::DeadLetter)
        );
        assert!("invalid".parse::<DeliveryStatus>().is_err());
    }

    // --- Event type validation tests ---

    #[test]
    fn test_validate_event_types_valid() {
        let types = vec![
            "account.opened".to_string(),
            "transaction.completed".to_string(),
        ];
        assert!(validate_event_types(&types).is_ok());
    }

    #[test]
    fn test_validate_event_types_all_valid() {
        let types: Vec<String> = VALID_EVENT_TYPES.iter().map(|s| s.to_string()).collect();
        assert!(validate_event_types(&types).is_ok());
    }

    #[test]
    fn test_validate_event_types_invalid() {
        let types = vec!["account.opened".to_string(), "invalid.event".to_string()];
        let result = validate_event_types(&types);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), vec!["invalid.event".to_string()]);
    }

    #[test]
    fn test_validate_event_types_empty() {
        let types: Vec<String> = vec![];
        assert!(validate_event_types(&types).is_ok());
    }

    #[test]
    fn test_validate_event_types_transaction_initiated() {
        let types = vec!["transaction.initiated".to_string()];
        assert!(validate_event_types(&types).is_ok());
    }

    #[test]
    fn test_valid_event_types_contains_transaction_initiated() {
        assert!(VALID_EVENT_TYPES.contains(&"transaction.initiated"));
    }

    #[test]
    fn test_validate_all_transaction_event_types() {
        let types = vec![
            "transaction.initiated".to_string(),
            "transaction.completed".to_string(),
            "transaction.failed".to_string(),
        ];
        assert!(validate_event_types(&types).is_ok());
    }

    #[test]
    fn test_valid_event_types_count() {
        assert_eq!(VALID_EVENT_TYPES.len(), 7);
    }

    // --- Signing secret tests ---

    #[test]
    fn test_generate_signing_secret_format() {
        let secret = generate_signing_secret();
        assert!(secret.starts_with("whsec_"));
        // whsec_ (6) + 64 hex chars = 70
        assert_eq!(secret.len(), 70);
    }

    #[test]
    fn test_generate_signing_secret_uniqueness() {
        let s1 = generate_signing_secret();
        let s2 = generate_signing_secret();
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_mask_signing_secret() {
        let secret = "whsec_a1b2c3d4e5f6g7h8";
        let masked = mask_signing_secret(secret);
        assert_eq!(masked, "whsec_a1b2****");
    }

    #[test]
    fn test_mask_signing_secret_short() {
        let masked = mask_signing_secret("short");
        assert_eq!(masked, "whsec_****");
    }

    // --- Serialization tests ---

    #[test]
    fn test_signing_secret_not_serialized_in_endpoint() {
        let endpoint = WebhookEndpoint {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            url: "https://example.com/webhook".to_string(),
            signing_secret: "whsec_secret_value".to_string(),
            event_types: vec!["account.opened".to_string()],
            description: Some("Test".to_string()),
            status: EndpointStatus::Active,
            failure_count: 0,
            disabled_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&endpoint).unwrap();
        assert!(!json.contains("whsec_secret_value"));
        assert!(!json.contains("signing_secret"));
    }

    #[test]
    fn test_webhook_payload_type_field_renamed() {
        let payload = WebhookPayload {
            id: "evt_123".to_string(),
            event_type: "account.opened".to_string(),
            created_at: Utc::now(),
            tenant_id: 100,
            data: serde_json::json!({"account_id": "abc"}),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"type\":\"account.opened\""));
        assert!(!json.contains("\"event_type\""));
    }

    #[test]
    fn test_create_endpoint_request_deserialization() {
        let json = r#"{"url":"https://example.com","event_types":["account.opened"],"description":"Test"}"#;
        let req: CreateEndpointRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com");
        assert_eq!(req.event_types, vec!["account.opened"]);
        assert_eq!(req.description, Some("Test".to_string()));
    }

    #[test]
    fn test_create_endpoint_request_without_description() {
        let json = r#"{"url":"https://example.com","event_types":["account.opened"]}"#;
        let req: CreateEndpointRequest = serde_json::from_str(json).unwrap();
        assert!(req.description.is_none());
    }

    #[test]
    fn test_update_endpoint_request_partial() {
        let json = r#"{"url":"https://new.example.com"}"#;
        let req: UpdateEndpointRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, Some("https://new.example.com".to_string()));
        assert!(req.event_types.is_none());
        assert!(req.description.is_none());
        assert!(req.status.is_none());
    }
}
