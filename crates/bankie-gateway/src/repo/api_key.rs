use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::RepoError;
use crate::models::api_key::{ApiKey, KeyStatus};

#[allow(clippy::too_many_arguments)]
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        tenant_id: i32,
        name: String,
        key_prefix: String,
        key_hash: String,
        scopes: Vec<String>,
    ) -> Result<ApiKey, RepoError>;

    async fn find_by_id(&self, id: Uuid, org_id: Uuid) -> Result<Option<ApiKey>, RepoError>;

    async fn find_valid_by_hash(&self, key_hash: String) -> Result<Option<ApiKey>, RepoError>;

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<ApiKey>, RepoError>;

    async fn update_status(
        &self,
        id: Uuid,
        status: KeyStatus,
        grace_expires_at: Option<DateTime<Utc>>,
    ) -> Result<Option<ApiKey>, RepoError>;
}
