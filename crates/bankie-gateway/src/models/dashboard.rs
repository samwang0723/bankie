use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// A single audit log entry returned by the activity endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct AuditLogEntry {
    pub id: i64,
    pub org_id: Uuid,
    pub actor_id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub changes: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

/// Parameters for inserting a new audit log entry.
#[derive(Debug)]
pub struct NewAuditLog {
    pub org_id: Uuid,
    pub actor_id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub changes: Option<serde_json::Value>,
    pub client_ip: Option<String>,
}
