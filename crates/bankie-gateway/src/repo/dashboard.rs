use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::RepoError;
use crate::models::dashboard::{AuditLogEntry, NewAuditLog};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait DashboardRepository: Send + Sync {
    /// Count API calls for an org since a given timestamp.
    async fn count_api_calls_since(
        &self,
        org_id: Uuid,
        since: DateTime<Utc>,
    ) -> Result<i64, RepoError>;

    /// Count API calls per key since a given timestamp.
    async fn count_api_calls_per_key_since(
        &self,
        key_ids: Vec<Uuid>,
        since: DateTime<Utc>,
    ) -> Result<HashMap<Uuid, i64>, RepoError>;

    /// List recent activity (audit log entries) for an org.
    async fn list_recent_activity(
        &self,
        org_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, RepoError>;

    /// Insert a new audit log entry.
    async fn insert_audit_log(&self, entry: NewAuditLog) -> Result<(), RepoError>;
}
