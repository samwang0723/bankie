use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::RepoError;
use crate::models::member::OrgMember;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
#[allow(clippy::too_many_arguments)]
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

    /// Create a member with a specific status (e.g., 'pending' for invites),
    /// optional invite token hash, and invite expiry.
    async fn create_with_status(
        &self,
        id: Uuid,
        org_id: Uuid,
        email: String,
        password_hash: String,
        role: String,
        status: String,
        invite_token_hash: Option<String>,
        invite_expires_at: Option<DateTime<Utc>>,
    ) -> Result<OrgMember, RepoError>;

    /// Find a member by their invite token hash.
    async fn find_by_invite_token_hash(&self, hash: String)
        -> Result<Option<OrgMember>, RepoError>;

    /// Accept an invite: set password, status=active, clear invite token fields.
    async fn accept_invite(
        &self,
        id: Uuid,
        password_hash: String,
    ) -> Result<Option<OrgMember>, RepoError>;

    /// Update invite token and expiry for an existing pending member.
    async fn update_invite_token(
        &self,
        id: Uuid,
        invite_token_hash: String,
        invite_expires_at: DateTime<Utc>,
    ) -> Result<Option<OrgMember>, RepoError>;
}
