use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::config::DbPools;
use uuid::Uuid;

use super::RepoError;
use crate::models::api_key::{ApiKey, KeyStatus};
use crate::models::dashboard::{AuditLogEntry, AuditLogFilters, NewAuditLog};
use crate::models::member::{MemberRole, MemberStatus, OrgMember};
use crate::models::org::{OrgStatus, Organization};
use crate::models::webhook::{
    ApiLogEntry, DeliveryStatus, EndpointStatus, PendingDelivery, WebhookDelivery, WebhookEndpoint,
    WebhookEvent,
};

// ─── Row types for sqlx deserialization ───

#[derive(sqlx::FromRow)]
struct OrgRow {
    id: Uuid,
    tenant_id: i32,
    name: String,
    slug: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<OrgRow> for Organization {
    fn from(row: OrgRow) -> Self {
        Organization {
            id: row.id,
            tenant_id: row.tenant_id,
            name: row.name,
            slug: row.slug,
            status: match row.status.as_str() {
                "suspended" => OrgStatus::Suspended,
                "closed" => OrgStatus::Closed,
                _ => OrgStatus::Active,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    email: String,
    password_hash: String,
    role: String,
    status: String,
    invite_token_hash: Option<String>,
    invite_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<MemberRow> for OrgMember {
    fn from(row: MemberRow) -> Self {
        OrgMember {
            id: row.id,
            org_id: row.org_id,
            name: row.name,
            email: row.email,
            password_hash: row.password_hash,
            role: match row.role.as_str() {
                "owner" => MemberRole::Owner,
                "admin" => MemberRole::Admin,
                _ => MemberRole::Member,
            },
            status: match row.status.as_str() {
                "pending" => MemberStatus::Pending,
                "suspended" => MemberStatus::Suspended,
                _ => MemberStatus::Active,
            },
            invite_token_hash: row.invite_token_hash,
            invite_expires_at: row.invite_expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    org_id: Uuid,
    tenant_id: i32,
    name: String,
    key_prefix: String,
    key_hash: String,
    scopes: serde_json::Value,
    status: String,
    grace_expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ApiKeyRow> for ApiKey {
    fn from(row: ApiKeyRow) -> Self {
        let scopes: Vec<String> = serde_json::from_value(row.scopes).unwrap_or_default();
        ApiKey {
            id: row.id,
            org_id: row.org_id,
            tenant_id: row.tenant_id,
            name: row.name,
            key_prefix: row.key_prefix,
            key_hash: row.key_hash,
            scopes,
            status: match row.status.as_str() {
                "rotated" => KeyStatus::Rotated,
                "revoked" => KeyStatus::Revoked,
                _ => KeyStatus::Active,
            },
            grace_expires_at: row.grace_expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn status_str(status: &KeyStatus) -> &'static str {
    match status {
        KeyStatus::Active => "active",
        KeyStatus::Rotated => "rotated",
        KeyStatus::Revoked => "revoked",
    }
}

// ─── PgOrgRepository ───

pub struct PgOrgRepository {
    write_pool: PgPool,
    read_pool: PgPool,
}

impl PgOrgRepository {
    pub fn new(pools: &DbPools) -> Self {
        Self {
            write_pool: pools.write().clone(),
            read_pool: pools.read().clone(),
        }
    }
}

#[async_trait]
impl super::org::OrgRepository for PgOrgRepository {
    async fn create(
        &self,
        id: Uuid,
        _tenant_id: i32,
        name: String,
        slug: String,
    ) -> Result<Organization, RepoError> {
        // tenant_id is auto-assigned by the portal.tenant_id_seq sequence
        let row: OrgRow = sqlx::query_as(
            r#"
            INSERT INTO portal.organizations (id, name, slug)
            VALUES ($1, $2, $3)
            RETURNING id, tenant_id, name, slug, status, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&name)
        .bind(&slug)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Organization>, RepoError> {
        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, name, slug, status, created_at, updated_at
            FROM portal.organizations
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }

    async fn find_by_slug(&self, slug: String) -> Result<Option<Organization>, RepoError> {
        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, name, slug, status, created_at, updated_at
            FROM portal.organizations
            WHERE slug = $1
            "#,
        )
        .bind(&slug)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }

    async fn update(
        &self,
        id: Uuid,
        name: Option<String>,
        status: Option<OrgStatus>,
    ) -> Result<Option<Organization>, RepoError> {
        let status_str = status.map(|s| match s {
            OrgStatus::Active => "active",
            OrgStatus::Suspended => "suspended",
            OrgStatus::Closed => "closed",
        });

        let row: Option<OrgRow> = sqlx::query_as(
            r#"
            UPDATE portal.organizations
            SET name = COALESCE($2, name),
                status = COALESCE($3, status),
                updated_at = now()
            WHERE id = $1
            RETURNING id, tenant_id, name, slug, status, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(name)
        .bind(status_str)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(Organization::from))
    }
}

// ─── PgMemberRepository ───

pub struct PgMemberRepository {
    write_pool: PgPool,
    read_pool: PgPool,
}

impl PgMemberRepository {
    pub fn new(pools: &DbPools) -> Self {
        Self {
            write_pool: pools.write().clone(),
            read_pool: pools.read().clone(),
        }
    }
}

#[async_trait]
impl super::member::MemberRepository for PgMemberRepository {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        name: String,
        email: String,
        password_hash: String,
        role: String,
    ) -> Result<OrgMember, RepoError> {
        let row: MemberRow = sqlx::query_as(
            r#"
            INSERT INTO portal.org_members (id, org_id, name, email, password_hash, role, status)
            VALUES ($1, $2, $3, $4, $5, $6, 'active')
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&name)
        .bind(&email)
        .bind(&password_hash)
        .bind(&role)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key")
                || e.to_string().contains("unique constraint")
            {
                RepoError::Conflict("Email already registered".to_string())
            } else {
                RepoError::Database(e.to_string())
            }
        })?;

        Ok(row.into())
    }

    async fn find_by_email(&self, email: String) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, name, email, password_hash, role, status,
                   invite_token_hash, invite_expires_at, created_at, updated_at
            FROM portal.org_members
            WHERE email = $1
            "#,
        )
        .bind(&email)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, name, email, password_hash, role, status,
                   invite_token_hash, invite_expires_at, created_at, updated_at
            FROM portal.org_members
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<OrgMember>, RepoError> {
        let rows: Vec<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, name, email, password_hash, role, status,
                   invite_token_hash, invite_expires_at, created_at, updated_at
            FROM portal.org_members
            WHERE org_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(org_id)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(OrgMember::from).collect())
    }

    async fn find_by_id_and_org(
        &self,
        id: Uuid,
        org_id: Uuid,
    ) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, name, email, password_hash, role, status,
                   invite_token_hash, invite_expires_at, created_at, updated_at
            FROM portal.org_members
            WHERE id = $1 AND org_id = $2
            "#,
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn update_role(&self, id: Uuid, role: String) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            UPDATE portal.org_members
            SET role = $2, updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&role)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: String,
    ) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            UPDATE portal.org_members
            SET status = $2, updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&status)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn delete(&self, id: Uuid) -> Result<bool, RepoError> {
        let result = sqlx::query(
            r#"
            DELETE FROM portal.org_members
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn create_with_status(
        &self,
        id: Uuid,
        org_id: Uuid,
        name: String,
        email: String,
        password_hash: String,
        role: String,
        status: String,
        invite_token_hash: Option<String>,
        invite_expires_at: Option<DateTime<Utc>>,
    ) -> Result<OrgMember, RepoError> {
        let row: MemberRow = sqlx::query_as(
            r#"
            INSERT INTO portal.org_members (id, org_id, name, email, password_hash, role, status, invite_token_hash, invite_expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&name)
        .bind(&email)
        .bind(&password_hash)
        .bind(&role)
        .bind(&status)
        .bind(invite_token_hash.as_deref())
        .bind(invite_expires_at)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key")
                || e.to_string().contains("unique constraint")
            {
                RepoError::Conflict("Email already registered".to_string())
            } else {
                RepoError::Database(e.to_string())
            }
        })?;

        Ok(row.into())
    }

    async fn find_by_invite_token_hash(
        &self,
        hash: String,
    ) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, name, email, password_hash, role, status,
                   invite_token_hash, invite_expires_at, created_at, updated_at
            FROM portal.org_members
            WHERE invite_token_hash = $1
            "#,
        )
        .bind(&hash)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn accept_invite(
        &self,
        id: Uuid,
        name: String,
        password_hash: String,
    ) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            UPDATE portal.org_members
            SET name = $2,
                password_hash = $3,
                status = 'active',
                invite_token_hash = NULL,
                invite_expires_at = NULL,
                updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&name)
        .bind(&password_hash)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }

    async fn update_invite_token(
        &self,
        id: Uuid,
        invite_token_hash: String,
        invite_expires_at: DateTime<Utc>,
    ) -> Result<Option<OrgMember>, RepoError> {
        let row: Option<MemberRow> = sqlx::query_as(
            r#"
            UPDATE portal.org_members
            SET invite_token_hash = $2,
                invite_expires_at = $3,
                updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, name, email, password_hash, role, status,
                      invite_token_hash, invite_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&invite_token_hash)
        .bind(invite_expires_at)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(OrgMember::from))
    }
}

// ─── PgApiKeyRepository ───

pub struct PgApiKeyRepository {
    write_pool: PgPool,
    read_pool: PgPool,
}

impl PgApiKeyRepository {
    pub fn new(pools: &DbPools) -> Self {
        Self {
            write_pool: pools.write().clone(),
            read_pool: pools.read().clone(),
        }
    }
}

#[async_trait]
impl super::api_key::ApiKeyRepository for PgApiKeyRepository {
    async fn create(
        &self,
        id: Uuid,
        org_id: Uuid,
        tenant_id: i32,
        name: String,
        key_prefix: String,
        key_hash: String,
        scopes: Vec<String>,
    ) -> Result<ApiKey, RepoError> {
        let scopes_json = serde_json::to_value(&scopes).unwrap_or_default();

        let row: ApiKeyRow = sqlx::query_as(
            r#"
            INSERT INTO portal.api_keys (id, org_id, tenant_id, name, key_prefix, key_hash, scopes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                      status, grace_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(tenant_id)
        .bind(&name)
        .bind(&key_prefix)
        .bind(&key_hash)
        .bind(&scopes_json)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(&self, id: Uuid, org_id: Uuid) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE id = $1 AND org_id = $2
            "#,
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }

    async fn find_valid_by_hash(&self, key_hash: String) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE key_hash = $1 AND status = 'active'
            "#,
        )
        .bind(&key_hash)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }

    async fn list_by_org(&self, org_id: Uuid) -> Result<Vec<ApiKey>, RepoError> {
        let rows: Vec<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE org_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(org_id)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(ApiKey::from).collect())
    }

    async fn update_status(
        &self,
        id: Uuid,
        status: KeyStatus,
        grace_expires_at: Option<DateTime<Utc>>,
    ) -> Result<Option<ApiKey>, RepoError> {
        let row: Option<ApiKeyRow> = sqlx::query_as(
            r#"
            UPDATE portal.api_keys
            SET status = $2, grace_expires_at = $3, updated_at = now()
            WHERE id = $1
            RETURNING id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                      status, grace_expires_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(status_str(&status))
        .bind(grace_expires_at)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(ApiKey::from))
    }

    async fn list_expired_rotated(&self) -> Result<Vec<ApiKey>, RepoError> {
        let rows: Vec<ApiKeyRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, tenant_id, name, key_prefix, key_hash, scopes,
                   status, grace_expires_at, created_at, updated_at
            FROM portal.api_keys
            WHERE status = 'rotated'
              AND grace_expires_at IS NOT NULL
              AND grace_expires_at < now()
            "#,
        )
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(ApiKey::from).collect())
    }
}

// ─── PgDashboardRepository ───

#[derive(sqlx::FromRow)]
struct AuditLogRow {
    id: i64,
    org_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    changes: Option<serde_json::Value>,
    client_ip: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<AuditLogRow> for AuditLogEntry {
    fn from(row: AuditLogRow) -> Self {
        AuditLogEntry {
            id: row.id,
            org_id: row.org_id.unwrap_or_default(),
            actor_id: row.actor_id.unwrap_or_default(),
            action: row.action,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            changes: row.changes,
            client_ip: row.client_ip,
            created_at: row.created_at,
        }
    }
}

pub struct PgDashboardRepository {
    write_pool: PgPool,
    read_pool: PgPool,
}

impl PgDashboardRepository {
    pub fn new(pools: &DbPools) -> Self {
        Self {
            write_pool: pools.write().clone(),
            read_pool: pools.read().clone(),
        }
    }
}

#[async_trait]
impl super::dashboard::DashboardRepository for PgDashboardRepository {
    async fn count_api_calls_since(
        &self,
        org_id: Uuid,
        since: DateTime<Utc>,
    ) -> Result<i64, RepoError> {
        // Count API logs for keys belonging to this org since the given timestamp.
        let row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) as count
            FROM portal.api_logs al
            JOIN portal.api_keys ak ON ak.id = al.api_key_id
            WHERE ak.org_id = $1 AND al.created_at >= $2
            "#,
        )
        .bind(org_id)
        .bind(since)
        .fetch_one(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.0)
    }

    async fn count_api_calls_per_key_since(
        &self,
        key_ids: Vec<Uuid>,
        since: DateTime<Utc>,
    ) -> Result<std::collections::HashMap<Uuid, i64>, super::RepoError> {
        let rows: Vec<(Uuid, i64)> = sqlx::query_as(
            r#"
            SELECT api_key_id, COUNT(*) as count
            FROM portal.api_logs
            WHERE api_key_id = ANY($1) AND created_at >= $2
            GROUP BY api_key_id
            "#,
        )
        .bind(&key_ids)
        .bind(since)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().collect())
    }

    async fn list_recent_activity(
        &self,
        org_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AuditLogEntry>, RepoError> {
        let rows: Vec<AuditLogRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, actor_id, action, resource_type, resource_id, changes, client_ip, created_at
            FROM portal.audit_logs
            WHERE org_id = $1
            ORDER BY created_at DESC
            LIMIT $2
            "#,
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(AuditLogEntry::from).collect())
    }

    async fn insert_audit_log(&self, entry: NewAuditLog) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            INSERT INTO portal.audit_logs (org_id, actor_id, action, resource_type, resource_id, changes, client_ip)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(entry.org_id)
        .bind(entry.actor_id)
        .bind(&entry.action)
        .bind(&entry.resource_type)
        .bind(entry.resource_id.as_deref())
        .bind(&entry.changes)
        .bind(entry.client_ip.as_deref())
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_audit_logs(
        &self,
        org_id: Uuid,
        filters: AuditLogFilters,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<AuditLogEntry>, i64), RepoError> {
        // Count query with optional filters
        let count_row: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM portal.audit_logs
            WHERE org_id = $1
              AND ($2::text IS NULL OR action = $2)
              AND ($3::timestamptz IS NULL OR created_at >= $3)
              AND ($4::timestamptz IS NULL OR created_at <= $4)
            "#,
        )
        .bind(org_id)
        .bind(filters.action.as_deref())
        .bind(filters.from)
        .bind(filters.to)
        .fetch_one(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        let total = count_row.0;

        // Data query with same filters + pagination
        let rows: Vec<AuditLogRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, actor_id, action, resource_type, resource_id, changes, client_ip, created_at
            FROM portal.audit_logs
            WHERE org_id = $1
              AND ($2::text IS NULL OR action = $2)
              AND ($3::timestamptz IS NULL OR created_at >= $3)
              AND ($4::timestamptz IS NULL OR created_at <= $4)
            ORDER BY created_at DESC
            LIMIT $5 OFFSET $6
            "#,
        )
        .bind(org_id)
        .bind(filters.action.as_deref())
        .bind(filters.from)
        .bind(filters.to)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok((rows.into_iter().map(AuditLogEntry::from).collect(), total))
    }
}

// ─── PgWebhookRepository ───

#[derive(sqlx::FromRow)]
struct WebhookEndpointRow {
    id: Uuid,
    org_id: Uuid,
    url: String,
    signing_secret: String,
    event_types: serde_json::Value,
    description: Option<String>,
    status: String,
    failure_count: i32,
    disabled_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<WebhookEndpointRow> for WebhookEndpoint {
    fn from(row: WebhookEndpointRow) -> Self {
        let event_types: Vec<String> = serde_json::from_value(row.event_types).unwrap_or_default();
        WebhookEndpoint {
            id: row.id,
            org_id: row.org_id,
            url: row.url,
            signing_secret: row.signing_secret,
            event_types,
            description: row.description,
            status: row
                .status
                .parse::<EndpointStatus>()
                .unwrap_or(EndpointStatus::Active),
            failure_count: row.failure_count,
            disabled_at: row.disabled_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct WebhookDeliveryRow {
    id: Uuid,
    endpoint_id: Uuid,
    event_type: String,
    event_source_id: String,
    payload: serde_json::Value,
    http_status: Option<i32>,
    attempt_number: i32,
    status: String,
    response_body: Option<String>,
    latency_ms: Option<i32>,
    next_retry_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl From<WebhookDeliveryRow> for WebhookDelivery {
    fn from(row: WebhookDeliveryRow) -> Self {
        WebhookDelivery {
            id: row.id,
            endpoint_id: row.endpoint_id,
            event_type: row.event_type,
            event_source_id: row.event_source_id,
            payload: row.payload,
            http_status: row.http_status,
            attempt_number: row.attempt_number,
            status: row
                .status
                .parse::<DeliveryStatus>()
                .unwrap_or(DeliveryStatus::Pending),
            response_body: row.response_body,
            latency_ms: row.latency_ms,
            next_retry_at: row.next_retry_at,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct WebhookEventRow {
    id: i64,
    tenant_id: i32,
    event_type: String,
    aggregate_type: String,
    aggregate_id: String,
    source_id: String,
    payload: serde_json::Value,
    processed: bool,
    created_at: DateTime<Utc>,
}

impl From<WebhookEventRow> for WebhookEvent {
    fn from(row: WebhookEventRow) -> Self {
        WebhookEvent {
            id: row.id,
            tenant_id: row.tenant_id,
            event_type: row.event_type,
            aggregate_type: row.aggregate_type,
            aggregate_id: row.aggregate_id,
            source_id: row.source_id,
            payload: row.payload,
            processed: row.processed,
            created_at: row.created_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ApiLogRow {
    id: i64,
    api_key_id: Option<Uuid>,
    tenant_id: Option<i32>,
    method: String,
    path: String,
    status_code: i32,
    latency_ms: Option<i32>,
    client_ip: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<ApiLogRow> for ApiLogEntry {
    fn from(row: ApiLogRow) -> Self {
        ApiLogEntry {
            id: row.id,
            api_key_id: row.api_key_id,
            tenant_id: row.tenant_id,
            method: row.method,
            path: row.path,
            status_code: row.status_code,
            latency_ms: row.latency_ms,
            client_ip: row.client_ip,
            created_at: row.created_at,
        }
    }
}

/// Joined row for pending deliveries (delivery + endpoint info).
#[derive(sqlx::FromRow)]
struct PendingDeliveryRow {
    // delivery fields
    id: Uuid,
    endpoint_id: Uuid,
    event_type: String,
    event_source_id: String,
    payload: serde_json::Value,
    http_status: Option<i32>,
    attempt_number: i32,
    status: String,
    response_body: Option<String>,
    latency_ms: Option<i32>,
    next_retry_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    // endpoint fields
    endpoint_url: String,
    signing_secret: String,
    endpoint_failure_count: i32,
}

impl From<PendingDeliveryRow> for PendingDelivery {
    fn from(row: PendingDeliveryRow) -> Self {
        PendingDelivery {
            delivery: WebhookDelivery {
                id: row.id,
                endpoint_id: row.endpoint_id,
                event_type: row.event_type,
                event_source_id: row.event_source_id,
                payload: row.payload,
                http_status: row.http_status,
                attempt_number: row.attempt_number,
                status: row
                    .status
                    .parse::<DeliveryStatus>()
                    .unwrap_or(DeliveryStatus::Pending),
                response_body: row.response_body,
                latency_ms: row.latency_ms,
                next_retry_at: row.next_retry_at,
                created_at: row.created_at,
            },
            endpoint_url: row.endpoint_url,
            signing_secret: row.signing_secret,
            endpoint_failure_count: row.endpoint_failure_count,
        }
    }
}

pub struct PgWebhookRepository {
    write_pool: PgPool,
    read_pool: PgPool,
}

impl PgWebhookRepository {
    pub fn new(pools: &DbPools) -> Self {
        Self {
            write_pool: pools.write().clone(),
            read_pool: pools.read().clone(),
        }
    }
}

#[async_trait]
impl super::webhook::WebhookRepository for PgWebhookRepository {
    // === Endpoint CRUD ===

    async fn create_endpoint(
        &self,
        id: Uuid,
        org_id: Uuid,
        url: String,
        signing_secret: String,
        event_types: Vec<String>,
        description: Option<String>,
    ) -> Result<WebhookEndpoint, RepoError> {
        let event_types_json = serde_json::to_value(&event_types).unwrap_or_default();
        let row: WebhookEndpointRow = sqlx::query_as(
            r#"
            INSERT INTO portal.webhook_endpoints (id, org_id, url, signing_secret, event_types, description)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, org_id, url, signing_secret, event_types, description,
                      status, failure_count, disabled_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&url)
        .bind(&signing_secret)
        .bind(&event_types_json)
        .bind(description.as_deref())
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_endpoint_by_id(
        &self,
        id: Uuid,
        org_id: Uuid,
    ) -> Result<Option<WebhookEndpoint>, RepoError> {
        let row: Option<WebhookEndpointRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, url, signing_secret, event_types, description,
                   status, failure_count, disabled_at, created_at, updated_at
            FROM portal.webhook_endpoints
            WHERE id = $1 AND org_id = $2
            "#,
        )
        .bind(id)
        .bind(org_id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(WebhookEndpoint::from))
    }

    async fn list_endpoints_by_org(&self, org_id: Uuid) -> Result<Vec<WebhookEndpoint>, RepoError> {
        let rows: Vec<WebhookEndpointRow> = sqlx::query_as(
            r#"
            SELECT id, org_id, url, signing_secret, event_types, description,
                   status, failure_count, disabled_at, created_at, updated_at
            FROM portal.webhook_endpoints
            WHERE org_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(org_id)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(WebhookEndpoint::from).collect())
    }

    async fn update_endpoint(
        &self,
        id: Uuid,
        org_id: Uuid,
        url: Option<String>,
        event_types: Option<Vec<String>>,
        description: Option<String>,
        status: Option<String>,
    ) -> Result<Option<WebhookEndpoint>, RepoError> {
        let event_types_json = event_types.map(|et| serde_json::to_value(&et).unwrap_or_default());
        let row: Option<WebhookEndpointRow> = sqlx::query_as(
            r#"
            UPDATE portal.webhook_endpoints
            SET url = COALESCE($3, url),
                event_types = COALESCE($4, event_types),
                description = COALESCE($5, description),
                status = COALESCE($6, status),
                failure_count = CASE WHEN $6 = 'active' THEN 0 ELSE failure_count END,
                disabled_at = CASE WHEN $6 = 'active' THEN NULL ELSE disabled_at END,
                updated_at = now()
            WHERE id = $1 AND org_id = $2
            RETURNING id, org_id, url, signing_secret, event_types, description,
                      status, failure_count, disabled_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(url.as_deref())
        .bind(event_types_json)
        .bind(description.as_deref())
        .bind(status.as_deref())
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(WebhookEndpoint::from))
    }

    async fn delete_endpoint(&self, id: Uuid, org_id: Uuid) -> Result<bool, RepoError> {
        // Delete deliveries first (FK constraint), then endpoint
        sqlx::query(r#"DELETE FROM portal.webhook_deliveries WHERE endpoint_id = $1"#)
            .bind(id)
            .execute(&self.write_pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?;

        let result =
            sqlx::query(r#"DELETE FROM portal.webhook_endpoints WHERE id = $1 AND org_id = $2"#)
                .bind(id)
                .bind(org_id)
                .execute(&self.write_pool)
                .await
                .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    // === Secret Rotation ===

    async fn rotate_signing_secret(
        &self,
        id: Uuid,
        org_id: Uuid,
        new_secret: String,
    ) -> Result<Option<WebhookEndpoint>, RepoError> {
        let row: Option<WebhookEndpointRow> = sqlx::query_as(
            r#"
            UPDATE portal.webhook_endpoints
            SET signing_secret = $3,
                updated_at = now()
            WHERE id = $1 AND org_id = $2
            RETURNING id, org_id, url, signing_secret, event_types, description,
                      status, failure_count, disabled_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(org_id)
        .bind(&new_secret)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(WebhookEndpoint::from))
    }

    // === Circuit Breaker ===

    async fn increment_failure_count(&self, id: Uuid) -> Result<i32, RepoError> {
        let row: (i32,) = sqlx::query_as(
            r#"
            UPDATE portal.webhook_endpoints
            SET failure_count = failure_count + 1, updated_at = now()
            WHERE id = $1
            RETURNING failure_count
            "#,
        )
        .bind(id)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.0)
    }

    async fn reset_failure_count(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            UPDATE portal.webhook_endpoints
            SET failure_count = 0, updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    async fn disable_endpoint(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            UPDATE portal.webhook_endpoints
            SET status = 'disabled', disabled_at = now(), updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    // === Fan-out Queries ===

    async fn find_active_endpoints_for_tenant(
        &self,
        tenant_id: i32,
        event_type: String,
    ) -> Result<Vec<WebhookEndpoint>, RepoError> {
        let rows: Vec<WebhookEndpointRow> = sqlx::query_as(
            r#"
            SELECT we.id, we.org_id, we.url, we.signing_secret, we.event_types, we.description,
                   we.status, we.failure_count, we.disabled_at, we.created_at, we.updated_at
            FROM portal.webhook_endpoints we
            JOIN portal.organizations o ON o.id = we.org_id
            WHERE o.tenant_id = $1
              AND we.status = 'active'
              AND we.event_types @> to_jsonb($2::text)
            "#,
        )
        .bind(tenant_id)
        .bind(&event_type)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(WebhookEndpoint::from).collect())
    }

    // === Webhook Events (staging table) ===

    async fn list_unprocessed_events(&self, limit: i64) -> Result<Vec<WebhookEvent>, RepoError> {
        let rows: Vec<WebhookEventRow> = sqlx::query_as(
            r#"
            SELECT id, tenant_id, event_type, aggregate_type, aggregate_id,
                   source_id, payload, processed, created_at
            FROM portal.webhook_events
            WHERE processed = false
            ORDER BY id ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(WebhookEvent::from).collect())
    }

    async fn mark_events_processed(&self, ids: Vec<i64>) -> Result<(), RepoError> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            r#"
            UPDATE portal.webhook_events
            SET processed = true
            WHERE id = ANY($1)
            "#,
        )
        .bind(&ids)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    // === Deliveries ===

    async fn create_delivery(
        &self,
        id: Uuid,
        endpoint_id: Uuid,
        event_type: String,
        event_source_id: String,
        payload: serde_json::Value,
    ) -> Result<WebhookDelivery, RepoError> {
        let row: WebhookDeliveryRow = sqlx::query_as(
            r#"
            INSERT INTO portal.webhook_deliveries
                (id, endpoint_id, event_type, event_source_id, payload, status, attempt_number, next_retry_at)
            VALUES ($1, $2, $3, $4, $5, 'pending', 1, now())
            ON CONFLICT (endpoint_id, event_source_id) WHERE event_source_id != '' DO NOTHING
            RETURNING id, endpoint_id, event_type, event_source_id, payload,
                      http_status, attempt_number, status, response_body,
                      latency_ms, next_retry_at, created_at
            "#,
        )
        .bind(id)
        .bind(endpoint_id)
        .bind(&event_type)
        .bind(&event_source_id)
        .bind(&payload)
        .fetch_one(&self.write_pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("duplicate key")
                || e.to_string().contains("unique constraint")
            {
                RepoError::Conflict("Duplicate delivery for this event".to_string())
            } else {
                RepoError::Database(e.to_string())
            }
        })?;

        Ok(row.into())
    }

    async fn list_pending_deliveries(&self, limit: i64) -> Result<Vec<PendingDelivery>, RepoError> {
        let rows: Vec<PendingDeliveryRow> = sqlx::query_as(
            r#"
            SELECT d.id, d.endpoint_id, d.event_type, d.event_source_id, d.payload,
                   d.http_status, d.attempt_number, d.status, d.response_body,
                   d.latency_ms, d.next_retry_at, d.created_at,
                   e.url AS endpoint_url, e.signing_secret,
                   e.failure_count AS endpoint_failure_count
            FROM portal.webhook_deliveries d
            JOIN portal.webhook_endpoints e ON d.endpoint_id = e.id
            WHERE d.status IN ('pending', 'failed')
              AND d.next_retry_at <= now()
              AND e.status = 'active'
            ORDER BY d.next_retry_at ASC
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(PendingDelivery::from).collect())
    }

    async fn update_delivery_success(
        &self,
        id: Uuid,
        http_status: i32,
        latency_ms: i32,
    ) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            UPDATE portal.webhook_deliveries
            SET status = 'success', http_status = $2, latency_ms = $3, next_retry_at = NULL
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(http_status)
        .bind(latency_ms)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    async fn update_delivery_failure(
        &self,
        id: Uuid,
        http_status: Option<i32>,
        latency_ms: Option<i32>,
        response_body: Option<String>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            UPDATE portal.webhook_deliveries
            SET status = 'failed',
                http_status = COALESCE($2, http_status),
                latency_ms = COALESCE($3, latency_ms),
                response_body = $4,
                attempt_number = attempt_number + 1,
                next_retry_at = $5
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(http_status)
        .bind(latency_ms)
        .bind(response_body.as_deref())
        .bind(next_retry_at)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    async fn move_to_dead_letter(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            r#"
            UPDATE portal.webhook_deliveries
            SET status = 'dead_letter', next_retry_at = NULL
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(())
    }

    async fn list_deliveries_by_endpoint(
        &self,
        endpoint_id: Uuid,
        page: i64,
        per_page: i64,
        status_filter: Option<String>,
    ) -> Result<(Vec<WebhookDelivery>, i64), RepoError> {
        let offset = (page - 1) * per_page;

        // Count total
        let (total,): (i64,) = if status_filter.is_some() {
            sqlx::query_as(
                r#"
                SELECT COUNT(*) FROM portal.webhook_deliveries
                WHERE endpoint_id = $1 AND status = $2
                "#,
            )
            .bind(endpoint_id)
            .bind(status_filter.as_deref())
            .fetch_one(&self.read_pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?
        } else {
            sqlx::query_as(
                r#"
                SELECT COUNT(*) FROM portal.webhook_deliveries
                WHERE endpoint_id = $1
                "#,
            )
            .bind(endpoint_id)
            .fetch_one(&self.read_pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?
        };

        // Fetch page
        let rows: Vec<WebhookDeliveryRow> = if status_filter.is_some() {
            sqlx::query_as(
                r#"
                SELECT id, endpoint_id, event_type, event_source_id, payload,
                       http_status, attempt_number, status, response_body,
                       latency_ms, next_retry_at, created_at
                FROM portal.webhook_deliveries
                WHERE endpoint_id = $1 AND status = $2
                ORDER BY created_at DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(endpoint_id)
            .bind(status_filter.as_deref())
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.read_pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?
        } else {
            sqlx::query_as(
                r#"
                SELECT id, endpoint_id, event_type, event_source_id, payload,
                       http_status, attempt_number, status, response_body,
                       latency_ms, next_retry_at, created_at
                FROM portal.webhook_deliveries
                WHERE endpoint_id = $1
                ORDER BY created_at DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(endpoint_id)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.read_pool)
            .await
            .map_err(|e| RepoError::Database(e.to_string()))?
        };

        Ok((rows.into_iter().map(WebhookDelivery::from).collect(), total))
    }

    async fn find_delivery_by_id(
        &self,
        id: Uuid,
        endpoint_id: Uuid,
    ) -> Result<Option<WebhookDelivery>, RepoError> {
        let row: Option<WebhookDeliveryRow> = sqlx::query_as(
            r#"
            SELECT id, endpoint_id, event_type, event_source_id, payload,
                   http_status, attempt_number, status, response_body,
                   latency_ms, next_retry_at, created_at
            FROM portal.webhook_deliveries
            WHERE id = $1 AND endpoint_id = $2
            "#,
        )
        .bind(id)
        .bind(endpoint_id)
        .fetch_optional(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(WebhookDelivery::from))
    }

    async fn reset_delivery_for_retry(
        &self,
        id: Uuid,
    ) -> Result<Option<WebhookDelivery>, RepoError> {
        let row: Option<WebhookDeliveryRow> = sqlx::query_as(
            r#"
            UPDATE portal.webhook_deliveries
            SET status = 'pending', attempt_number = 1, next_retry_at = now(),
                http_status = NULL, response_body = NULL, latency_ms = NULL
            WHERE id = $1 AND status = 'dead_letter'
            RETURNING id, endpoint_id, event_type, event_source_id, payload,
                      http_status, attempt_number, status, response_body,
                      latency_ms, next_retry_at, created_at
            "#,
        )
        .bind(id)
        .fetch_optional(&self.write_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok(row.map(WebhookDelivery::from))
    }

    // === API Logs ===

    async fn list_api_logs(
        &self,
        tenant_id: i32,
        filters: crate::models::webhook::ApiLogFilters,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<ApiLogEntry>, i64), RepoError> {
        let offset = (page - 1) * per_page;

        // Build dynamic WHERE clause parts
        let (total,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM portal.api_logs
            WHERE tenant_id = $1
              AND ($2::varchar IS NULL OR method = $2)
              AND ($3::int IS NULL OR status_code = $3)
              AND ($4::varchar IS NULL OR path LIKE '%' || $4 || '%')
              AND ($5::timestamptz IS NULL OR created_at >= $5)
              AND ($6::timestamptz IS NULL OR created_at <= $6)
            "#,
        )
        .bind(tenant_id)
        .bind(filters.method.as_deref())
        .bind(filters.status_code)
        .bind(filters.path.as_deref())
        .bind(filters.from)
        .bind(filters.to)
        .fetch_one(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        let rows: Vec<ApiLogRow> = sqlx::query_as(
            r#"
            SELECT id, api_key_id, tenant_id, method, path, status_code,
                   latency_ms, client_ip, created_at
            FROM portal.api_logs
            WHERE tenant_id = $1
              AND ($2::varchar IS NULL OR method = $2)
              AND ($3::int IS NULL OR status_code = $3)
              AND ($4::varchar IS NULL OR path LIKE '%' || $4 || '%')
              AND ($5::timestamptz IS NULL OR created_at >= $5)
              AND ($6::timestamptz IS NULL OR created_at <= $6)
            ORDER BY created_at DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(tenant_id)
        .bind(filters.method.as_deref())
        .bind(filters.status_code)
        .bind(filters.path.as_deref())
        .bind(filters.from)
        .bind(filters.to)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.read_pool)
        .await
        .map_err(|e| RepoError::Database(e.to_string()))?;

        Ok((rows.into_iter().map(ApiLogEntry::from).collect(), total))
    }
}
