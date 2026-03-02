use bankie_common::error::AppError;

use crate::models::auth::SessionClaims;
use crate::models::member::{parse_role, MemberRole};

/// Verify that the session role has permission to manage members.
/// Returns `Err(403)` if the role is not Owner or Admin.
pub fn require_member_management(claims: &SessionClaims) -> Result<MemberRole, AppError> {
    let role = parse_role(&claims.role);
    if !role.can_manage_members() {
        return Err(AppError::Forbidden(
            "Insufficient permissions: member management requires owner or admin role".to_string(),
        ));
    }
    Ok(role)
}

/// Verify that the session role has permission to manage API keys.
/// Returns `Err(403)` if the role is not Owner or Admin.
pub fn require_api_key_management(claims: &SessionClaims) -> Result<MemberRole, AppError> {
    let role = parse_role(&claims.role);
    if !role.can_manage_api_keys() {
        return Err(AppError::Forbidden(
            "Insufficient permissions: API key management requires owner or admin role".to_string(),
        ));
    }
    Ok(role)
}

/// Verify that the session role has permission to modify organization settings.
/// Returns `Err(403)` if the role is not Owner or Admin.
pub fn require_org_management(claims: &SessionClaims) -> Result<MemberRole, AppError> {
    let role = parse_role(&claims.role);
    if !role.can_manage_org() {
        return Err(AppError::Forbidden(
            "Insufficient permissions: organization management requires owner or admin role"
                .to_string(),
        ));
    }
    Ok(role)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims_with_role(role: &str) -> SessionClaims {
        SessionClaims {
            sub: uuid::Uuid::new_v4().to_string(),
            name: "Test User".to_string(),
            org_id: uuid::Uuid::new_v4().to_string(),
            tenant_id: 1,
            role: role.to_string(),
            csrf: "csrf".to_string(),
            exp: 9999999999,
        }
    }

    #[test]
    fn test_require_member_management_owner() {
        let claims = claims_with_role("owner");
        let result = require_member_management(&claims);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), MemberRole::Owner);
    }

    #[test]
    fn test_require_member_management_admin() {
        let claims = claims_with_role("admin");
        let result = require_member_management(&claims);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), MemberRole::Admin);
    }

    #[test]
    fn test_require_member_management_member_denied() {
        let claims = claims_with_role("member");
        let result = require_member_management(&claims);
        assert!(result.is_err());
    }

    #[test]
    fn test_require_api_key_management_owner() {
        let claims = claims_with_role("owner");
        assert!(require_api_key_management(&claims).is_ok());
    }

    #[test]
    fn test_require_api_key_management_admin() {
        let claims = claims_with_role("admin");
        assert!(require_api_key_management(&claims).is_ok());
    }

    #[test]
    fn test_require_api_key_management_member_denied() {
        let claims = claims_with_role("member");
        assert!(require_api_key_management(&claims).is_err());
    }

    #[test]
    fn test_require_org_management_member_denied() {
        let claims = claims_with_role("member");
        assert!(require_org_management(&claims).is_err());
    }

    #[test]
    fn test_require_org_management_admin() {
        let claims = claims_with_role("admin");
        assert!(require_org_management(&claims).is_ok());
    }
}
