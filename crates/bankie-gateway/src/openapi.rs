//! OpenAPI specification for the Bankie Gateway API.
//!
//! Uses utoipa to auto-generate the spec from Rust types.

#![allow(dead_code)]

use utoipa::OpenApi;

use crate::models::api_key::{
    CreateKeyRequest, CreateKeyResponse, KeyListItem, KeyStatus, RotateKeyResponse,
};
use crate::models::auth::{AuthOrganization, AuthResponse, AuthUser, LoginRequest, SignupRequest};
use crate::models::dashboard::AuditLogEntry;
use crate::models::member::{
    AcceptInviteRequest, InviteInfo, InviteMemberRequest, InviteMemberResponse, MemberRole,
    MemberStatus, OrgMember, UpdateRoleRequest,
};
use crate::models::org::{CreateOrgRequest, OrgStatus, Organization, UpdateOrgRequest};
use crate::models::webhook::{
    ApiLogEntry, CreateEndpointRequest, CreateEndpointResponse, DeliveryListItem, DeliveryStatus,
    EndpointListItem, EndpointStatus, RotateSecretResponse, UpdateEndpointRequest,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Bankie Gateway API",
        version = "1.0.0",
        description = "Developer Portal and API Gateway for the Bankie banking platform.\n\n\
            **Authentication:**\n\
            - Portal endpoints (`/portal/v1/*`): Session cookie auth + CSRF token\n\
            - External API (`/*`): Bearer API key auth\n\n\
            **Rate Limiting:** Token bucket (100 burst, 1000/min) per API key.",
        contact(name = "Bankie", url = "https://github.com/samwang0723/bankie")
    ),
    tags(
        (name = "auth", description = "Authentication: signup, login, logout, invite"),
        (name = "organization", description = "Organization management"),
        (name = "members", description = "Org member management and invites"),
        (name = "api-keys", description = "API key lifecycle: create, rotate, revoke"),
        (name = "dashboard", description = "Dashboard statistics and activity feed"),
        (name = "webhooks", description = "Webhook endpoint management and delivery history"),
        (name = "logs", description = "API request log viewer"),
        (name = "audit", description = "Audit log viewer"),
        (name = "data", description = "Data proxy to banking core (accounts, transactions, reports)")
    ),
    paths(
        // Auth
        path_signup,
        path_login,
        path_logout,
        path_validate_invite,
        path_accept_invite,
        // Organization
        path_get_org,
        path_update_org,
        // Members
        path_list_members,
        path_invite_member,
        path_change_role,
        path_remove_member,
        path_resend_invite,
        // API Keys
        path_list_keys,
        path_create_key,
        path_rotate_key,
        path_revoke_key,
        // Dashboard
        path_dashboard_stats,
        path_dashboard_activity,
        path_dashboard_rate_limits,
        // Webhooks
        path_list_endpoints,
        path_create_endpoint,
        path_update_endpoint,
        path_delete_endpoint,
        path_rotate_secret,
        path_list_deliveries,
        // Logs
        path_list_logs,
        // Audit
        path_list_audit_logs,
        // Data proxy
        path_list_accounts,
        path_get_account,
        path_get_sub_accounts,
        path_balance_history,
        path_list_house_accounts,
        path_list_transactions,
        path_get_ledger,
        path_settlement_report,
    ),
    components(schemas(
        // Auth
        SignupRequest, LoginRequest, AuthResponse, AuthUser, AuthOrganization,
        // Org
        Organization, OrgStatus, CreateOrgRequest, UpdateOrgRequest,
        // Members
        MemberRole, MemberStatus, OrgMember, InviteMemberRequest, InviteMemberResponse,
        AcceptInviteRequest, InviteInfo, UpdateRoleRequest,
        // API Keys
        KeyStatus, CreateKeyRequest, CreateKeyResponse, KeyListItem, RotateKeyResponse,
        // Webhooks
        EndpointStatus, DeliveryStatus, CreateEndpointRequest, UpdateEndpointRequest,
        CreateEndpointResponse, EndpointListItem, DeliveryListItem, RotateSecretResponse,
        ApiLogEntry,
        // Dashboard
        AuditLogEntry,
    ))
)]
pub struct ApiDoc;

// === Auth paths ===

#[utoipa::path(
    post, path = "/portal/v1/auth/signup",
    tag = "auth",
    request_body = SignupRequest,
    responses(
        (status = 200, description = "Signup successful, session cookie set", body = AuthResponse),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Email or org already exists")
    )
)]
async fn path_signup() {}

#[utoipa::path(
    post, path = "/portal/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful, session cookie set", body = AuthResponse),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many failed attempts")
    )
)]
async fn path_login() {}

#[utoipa::path(
    post, path = "/portal/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "Session cleared")
    )
)]
async fn path_logout() {}

#[utoipa::path(
    get, path = "/portal/v1/auth/invite",
    tag = "auth",
    params(("token" = String, Query, description = "Invite token")),
    responses(
        (status = 200, description = "Invite info", body = InviteInfo),
        (status = 404, description = "Invalid or expired token")
    )
)]
async fn path_validate_invite() {}

#[utoipa::path(
    post, path = "/portal/v1/auth/invite/accept",
    tag = "auth",
    request_body = AcceptInviteRequest,
    responses(
        (status = 200, description = "Invite accepted, session cookie set", body = AuthResponse),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Invalid token")
    )
)]
async fn path_accept_invite() {}

// === Organization paths ===

#[utoipa::path(
    get, path = "/portal/v1/organization",
    tag = "organization",
    security(("session" = [])),
    responses(
        (status = 200, description = "Organization details", body = Organization),
        (status = 404, description = "Not found")
    )
)]
async fn path_get_org() {}

#[utoipa::path(
    put, path = "/portal/v1/organization",
    tag = "organization",
    security(("session" = [])),
    request_body = UpdateOrgRequest,
    responses(
        (status = 200, description = "Organization updated", body = Organization),
        (status = 403, description = "Insufficient permissions")
    )
)]
async fn path_update_org() {}

// === Member paths ===

#[utoipa::path(
    get, path = "/portal/v1/members",
    tag = "members",
    security(("session" = [])),
    responses(
        (status = 200, description = "Member list", body = Vec<OrgMember>)
    )
)]
async fn path_list_members() {}

#[utoipa::path(
    post, path = "/portal/v1/members/invite",
    tag = "members",
    security(("session" = [])),
    request_body = InviteMemberRequest,
    responses(
        (status = 200, description = "Invite sent", body = InviteMemberResponse),
        (status = 403, description = "Insufficient permissions"),
        (status = 409, description = "Email already in org")
    )
)]
async fn path_invite_member() {}

#[utoipa::path(
    post, path = "/portal/v1/members/{id}/role",
    tag = "members",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Member ID")),
    request_body = UpdateRoleRequest,
    responses(
        (status = 200, description = "Role updated", body = OrgMember),
        (status = 403, description = "Insufficient permissions")
    )
)]
async fn path_change_role() {}

#[utoipa::path(
    delete, path = "/portal/v1/members/{id}",
    tag = "members",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Member ID")),
    responses(
        (status = 200, description = "Member removed"),
        (status = 403, description = "Insufficient permissions")
    )
)]
async fn path_remove_member() {}

#[utoipa::path(
    post, path = "/portal/v1/members/{id}/resend-invite",
    tag = "members",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Member ID")),
    responses(
        (status = 200, description = "Invite resent"),
        (status = 400, description = "Member not in pending status")
    )
)]
async fn path_resend_invite() {}

// === API Key paths ===

#[utoipa::path(
    get, path = "/portal/v1/api-keys",
    tag = "api-keys",
    security(("session" = [])),
    responses(
        (status = 200, description = "API key list", body = Vec<KeyListItem>)
    )
)]
async fn path_list_keys() {}

#[utoipa::path(
    post, path = "/portal/v1/api-keys",
    tag = "api-keys",
    security(("session" = [])),
    request_body = CreateKeyRequest,
    responses(
        (status = 200, description = "Key created (raw key shown once)", body = CreateKeyResponse),
        (status = 400, description = "Invalid scopes"),
        (status = 403, description = "Insufficient permissions")
    )
)]
async fn path_create_key() {}

#[utoipa::path(
    post, path = "/portal/v1/api-keys/{id}/rotate",
    tag = "api-keys",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "API key ID")),
    responses(
        (status = 200, description = "Key rotated with grace period", body = RotateKeyResponse),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Key not found")
    )
)]
async fn path_rotate_key() {}

#[utoipa::path(
    delete, path = "/portal/v1/api-keys/{id}",
    tag = "api-keys",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "API key ID")),
    responses(
        (status = 200, description = "Key revoked"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Key not found")
    )
)]
async fn path_revoke_key() {}

// === Dashboard paths ===

#[utoipa::path(
    get, path = "/portal/v1/dashboard/stats",
    tag = "dashboard",
    security(("session" = [])),
    responses(
        (status = 200, description = "Dashboard statistics")
    )
)]
async fn path_dashboard_stats() {}

#[utoipa::path(
    get, path = "/portal/v1/dashboard/activity",
    tag = "dashboard",
    security(("session" = [])),
    responses(
        (status = 200, description = "Recent activity feed", body = Vec<AuditLogEntry>)
    )
)]
async fn path_dashboard_activity() {}

#[utoipa::path(
    get, path = "/portal/v1/dashboard/rate-limits",
    tag = "dashboard",
    security(("session" = [])),
    responses(
        (status = 200, description = "Rate limit configuration and throttle counts")
    )
)]
async fn path_dashboard_rate_limits() {}

// === Webhook paths ===

#[utoipa::path(
    get, path = "/portal/v1/webhook-endpoints",
    tag = "webhooks",
    security(("session" = [])),
    responses(
        (status = 200, description = "Webhook endpoint list", body = Vec<EndpointListItem>)
    )
)]
async fn path_list_endpoints() {}

#[utoipa::path(
    post, path = "/portal/v1/webhook-endpoints",
    tag = "webhooks",
    security(("session" = [])),
    request_body = CreateEndpointRequest,
    responses(
        (status = 200, description = "Endpoint created (signing secret shown once)", body = CreateEndpointResponse),
        (status = 400, description = "Invalid URL or event types"),
        (status = 403, description = "Insufficient permissions")
    )
)]
async fn path_create_endpoint() {}

#[utoipa::path(
    put, path = "/portal/v1/webhook-endpoints/{id}",
    tag = "webhooks",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Endpoint ID")),
    request_body = UpdateEndpointRequest,
    responses(
        (status = 200, description = "Endpoint updated", body = EndpointListItem),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Not found")
    )
)]
async fn path_update_endpoint() {}

#[utoipa::path(
    delete, path = "/portal/v1/webhook-endpoints/{id}",
    tag = "webhooks",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, description = "Endpoint deleted"),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Not found")
    )
)]
async fn path_delete_endpoint() {}

#[utoipa::path(
    post, path = "/portal/v1/webhook-endpoints/{id}/rotate-secret",
    tag = "webhooks",
    security(("session" = [])),
    params(("id" = Uuid, Path, description = "Endpoint ID")),
    responses(
        (status = 200, description = "Secret rotated", body = RotateSecretResponse),
        (status = 403, description = "Insufficient permissions"),
        (status = 404, description = "Not found")
    )
)]
async fn path_rotate_secret() {}

#[utoipa::path(
    get, path = "/portal/v1/webhook-endpoints/{id}/deliveries",
    tag = "webhooks",
    security(("session" = [])),
    params(
        ("id" = Uuid, Path, description = "Endpoint ID"),
        ("status" = Option<String>, Query, description = "Filter by delivery status"),
        ("page" = Option<i64>, Query, description = "Page number"),
        ("per_page" = Option<i64>, Query, description = "Items per page")
    ),
    responses(
        (status = 200, description = "Delivery list", body = Vec<DeliveryListItem>)
    )
)]
async fn path_list_deliveries() {}

// === Logs paths ===

#[utoipa::path(
    get, path = "/portal/v1/logs",
    tag = "logs",
    security(("session" = [])),
    params(
        ("method" = Option<String>, Query, description = "Filter by HTTP method"),
        ("status_code" = Option<i32>, Query, description = "Filter by status code"),
        ("path" = Option<String>, Query, description = "Filter by path prefix"),
        ("page" = Option<i64>, Query, description = "Page number"),
        ("per_page" = Option<i64>, Query, description = "Items per page")
    ),
    responses(
        (status = 200, description = "API log entries", body = Vec<ApiLogEntry>)
    )
)]
async fn path_list_logs() {}

// === Audit paths ===

#[utoipa::path(
    get, path = "/portal/v1/audit-logs",
    tag = "audit",
    security(("session" = [])),
    params(
        ("action" = Option<String>, Query, description = "Filter by action"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination"),
        ("limit" = Option<i64>, Query, description = "Limit (max 100)"),
        ("from" = Option<String>, Query, description = "Start date (ISO 8601)"),
        ("to" = Option<String>, Query, description = "End date (ISO 8601)")
    ),
    responses(
        (status = 200, description = "Audit log entries", body = Vec<AuditLogEntry>)
    )
)]
async fn path_list_audit_logs() {}

// === Data proxy paths ===

#[utoipa::path(
    get, path = "/portal/v1/data/accounts",
    tag = "data",
    security(("session" = [])),
    params(
        ("offset" = Option<i64>, Query, description = "Offset"),
        ("limit" = Option<i64>, Query, description = "Limit")
    ),
    responses((status = 200, description = "Account list (proxied from Core)"))
)]
async fn path_list_accounts() {}

#[utoipa::path(
    get, path = "/portal/v1/data/accounts/{id}",
    tag = "data",
    security(("session" = [])),
    params(("id" = String, Path, description = "Account ID")),
    responses((status = 200, description = "Account details (proxied from Core)"))
)]
async fn path_get_account() {}

#[utoipa::path(
    get, path = "/portal/v1/data/accounts/{id}/sub-accounts",
    tag = "data",
    security(("session" = [])),
    params(("id" = String, Path, description = "Parent account ID")),
    responses((status = 200, description = "Sub-account list (proxied from Core)"))
)]
async fn path_get_sub_accounts() {}

#[utoipa::path(
    get, path = "/portal/v1/data/accounts/{id}/balance-history",
    tag = "data",
    security(("session" = [])),
    params(
        ("id" = String, Path, description = "Account ID"),
        ("start_date" = Option<String>, Query, description = "Start date"),
        ("end_date" = Option<String>, Query, description = "End date")
    ),
    responses((status = 200, description = "Balance history (proxied from Core)"))
)]
async fn path_balance_history() {}

#[utoipa::path(
    get, path = "/portal/v1/data/house-accounts",
    tag = "data",
    security(("session" = [])),
    params(("currency" = Option<String>, Query, description = "Filter by currency")),
    responses((status = 200, description = "House account list (proxied from Core)"))
)]
async fn path_list_house_accounts() {}

#[utoipa::path(
    get, path = "/portal/v1/data/transactions",
    tag = "data",
    security(("session" = [])),
    params(
        ("bank_account_id" = Option<String>, Query, description = "Filter by account"),
        ("offset" = Option<i64>, Query, description = "Offset"),
        ("limit" = Option<i64>, Query, description = "Limit"),
        ("start_date" = Option<String>, Query, description = "Start date"),
        ("end_date" = Option<String>, Query, description = "End date"),
        ("transaction_type" = Option<String>, Query, description = "Filter by type"),
        ("status" = Option<String>, Query, description = "Filter by status")
    ),
    responses((status = 200, description = "Transaction list (proxied from Core)"))
)]
async fn path_list_transactions() {}

#[utoipa::path(
    get, path = "/portal/v1/data/ledger/{id}",
    tag = "data",
    security(("session" = [])),
    params(("id" = String, Path, description = "Ledger ID")),
    responses((status = 200, description = "Ledger details (proxied from Core)"))
)]
async fn path_get_ledger() {}

#[utoipa::path(
    get, path = "/portal/v1/data/reports/settlement",
    tag = "data",
    security(("session" = [])),
    params(
        ("start_date" = Option<String>, Query, description = "Start date"),
        ("end_date" = Option<String>, Query, description = "End date"),
        ("bank_account_id" = Option<String>, Query, description = "Filter by account"),
        ("currency" = Option<String>, Query, description = "Filter by currency")
    ),
    responses((status = 200, description = "Settlement report CSV (proxied from Core)"))
)]
async fn path_settlement_report() {}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    #[test]
    fn test_openapi_spec_generates_valid_json() {
        let spec = ApiDoc::openapi();
        let json = serde_json::to_string_pretty(&spec).unwrap();
        assert!(json.contains("Bankie Gateway API"), "missing title");
        assert!(json.contains("1.0.0"), "missing version");
        assert!(json.contains("openapi"), "missing openapi field");
    }

    #[test]
    fn test_openapi_spec_has_all_tags() {
        let spec = ApiDoc::openapi();
        let json = serde_json::to_string(&spec).unwrap();
        for tag in &[
            "auth",
            "organization",
            "members",
            "api-keys",
            "dashboard",
            "webhooks",
            "logs",
            "audit",
            "data",
        ] {
            assert!(json.contains(tag), "missing tag: {tag}");
        }
    }

    #[test]
    fn test_openapi_spec_has_paths() {
        let spec = ApiDoc::openapi();
        let paths = spec.paths;
        assert!(
            paths.paths.len() >= 30,
            "expected >= 30 paths, got {}",
            paths.paths.len()
        );
    }

    #[test]
    fn test_openapi_spec_schemas_registered() {
        let spec = ApiDoc::openapi();
        let json = serde_json::to_string(&spec).unwrap();
        for schema in &[
            "SignupRequest",
            "LoginRequest",
            "AuthResponse",
            "Organization",
            "OrgMember",
            "KeyListItem",
            "CreateEndpointRequest",
            "AuditLogEntry",
            "ApiLogEntry",
        ] {
            assert!(json.contains(schema), "missing schema: {schema}");
        }
    }

    #[test]
    fn test_openapi_spec_excludes_sensitive_fields() {
        let spec = ApiDoc::openapi();
        let json = serde_json::to_string(&spec).unwrap();
        // password_hash, key_hash, signing_secret should be excluded via #[schema(ignore)]
        assert!(
            !json.contains("password_hash"),
            "password_hash should be excluded"
        );
        assert!(!json.contains("key_hash"), "key_hash should be excluded");
    }

    #[test]
    fn test_openapi_spec_security_scheme() {
        let spec = ApiDoc::openapi();
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("session"), "missing session security scheme");
        assert!(json.contains("api_key"), "missing api_key security scheme");
    }
}
