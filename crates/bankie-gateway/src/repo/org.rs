use async_trait::async_trait;
use uuid::Uuid;

use super::RepoError;
use crate::models::org::{OrgStatus, Organization};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OrgRepository: Send + Sync {
    async fn create(
        &self,
        id: Uuid,
        tenant_id: i32,
        name: String,
        slug: String,
    ) -> Result<Organization, RepoError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Organization>, RepoError>;

    async fn find_by_slug(&self, slug: String) -> Result<Option<Organization>, RepoError>;

    async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        status: Option<OrgStatus>,
    ) -> Result<Option<Organization>, RepoError>;
}
