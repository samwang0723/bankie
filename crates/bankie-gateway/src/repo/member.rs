use async_trait::async_trait;
use uuid::Uuid;

use super::RepoError;
use crate::models::member::OrgMember;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait MemberRepository: Send + Sync {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        email: String,
        password_hash: String,
        role: String,
    ) -> Result<OrgMember, RepoError>;

    async fn find_by_email(&self, email: String) -> Result<Option<OrgMember>, RepoError>;

    async fn find_by_id(&self, id: Uuid) -> Result<Option<OrgMember>, RepoError>;
}
