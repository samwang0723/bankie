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

    /// List all members belonging to an organization.
    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<OrgMember>, RepoError>;

    /// Find a member by ID scoped to a specific organization.
    async fn find_by_id_and_org(
        &self,
        id: Uuid,
        org_id: Uuid,
    ) -> Result<Option<OrgMember>, RepoError>;

    /// Update a member's role. Returns the updated member.
    async fn update_role(&self, id: Uuid, role: String) -> Result<Option<OrgMember>, RepoError>;

    /// Update a member's status. Returns the updated member.
    async fn update_status(&self, id: Uuid, status: String)
        -> Result<Option<OrgMember>, RepoError>;

    /// Delete a member by ID. Returns true if a row was deleted.
    async fn delete(&self, id: Uuid) -> Result<bool, RepoError>;

    /// Create a member with a specific status (e.g., 'pending' for invites).
    async fn create_with_status(
        &self,
        id: Uuid,
        org_id: Uuid,
        email: String,
        password_hash: String,
        role: String,
        status: String,
    ) -> Result<OrgMember, RepoError>;
}
