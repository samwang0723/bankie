use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    middleware::Next,
};
use chrono::Utc;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::repo::api_key::ResolvedApiKey;

/// Short-lived internal JWT TTL in seconds.
const INTERNAL_JWT_TTL_SECS: i64 = 60;

/// Claims structure matching bankie-core's JWT format.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: usize,
    pub iat: usize,
    #[serde(rename = "scope")]
    pub scopes: Vec<String>,
    pub tenant_id: i32,
}

/// Middleware that mints a short-lived internal JWT from the resolved API key.
///
/// Reads `ResolvedApiKey` from extensions (set by api_key_resolver),
/// mints a 60-second JWT, and stores it in extensions for the proxy layer.
pub async fn jwt_minter(mut req: Request<Body>, next: Next) -> Result<Response<Body>, StatusCode> {
    let resolved = req
        .extensions()
        .get::<ResolvedApiKey>()
        .cloned()
        .ok_or_else(|| {
            error!("ResolvedApiKey not found in extensions — api_key_resolver must run first");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let jwt_secret = std::env::var("JWT_SECRET").map_err(|_| {
        error!("JWT_SECRET env var not set");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let token = mint_internal_jwt(&resolved, &jwt_secret).map_err(|e| {
        error!("Failed to mint internal JWT: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    req.extensions_mut().insert(InternalJwt(token));
    Ok(next.run(req).await)
}

/// Wrapper for the minted JWT token stored in request extensions.
#[derive(Debug, Clone)]
pub struct InternalJwt(pub String);

/// Mint a short-lived internal JWT for proxying to bankie-core.
pub fn mint_internal_jwt(
    resolved: &ResolvedApiKey,
    secret: &str,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now().timestamp();

    let claims = Claims {
        iss: "bankie-gateway".to_string(),
        sub: resolved.api_key_id.to_string(),
        aud: "service".to_string(),
        exp: (now + INTERNAL_JWT_TTL_SECS) as usize,
        iat: now as usize,
        scopes: resolved.scopes.clone(),
        tenant_id: resolved.tenant_id,
    };

    let header = Header::default();
    let encoding_key = EncodingKey::from_secret(secret.as_bytes());
    encode(&header, &claims, &encoding_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use uuid::Uuid;

    fn make_resolved() -> ResolvedApiKey {
        ResolvedApiKey {
            api_key_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            tenant_id: 42,
            scopes: vec![
                "bank-account:read".to_string(),
                "bank-account:write".to_string(),
            ],
            environment: "live".to_string(),
        }
    }

    #[test]
    fn test_mint_internal_jwt_success() {
        let resolved = make_resolved();
        let secret = "test-secret-key-for-jwt";

        let token = mint_internal_jwt(&resolved, secret).unwrap();
        assert!(!token.is_empty(), "JWT token should not be empty");
    }

    #[test]
    fn test_mint_internal_jwt_claims_match() {
        let resolved = make_resolved();
        let secret = "test-secret-key-for-jwt";

        let token = mint_internal_jwt(&resolved, secret).unwrap();

        // Decode and verify claims
        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        validation.set_issuer(&["bankie-gateway"]);

        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .unwrap();

        assert_eq!(decoded.claims.iss, "bankie-gateway");
        assert_eq!(decoded.claims.aud, "service");
        assert_eq!(decoded.claims.tenant_id, 42);
        assert_eq!(decoded.claims.sub, resolved.api_key_id.to_string());
        assert_eq!(decoded.claims.scopes, resolved.scopes);
    }

    #[test]
    fn test_mint_internal_jwt_short_lived() {
        let resolved = make_resolved();
        let secret = "test-secret-key";

        let token = mint_internal_jwt(&resolved, secret).unwrap();

        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        validation.insecure_disable_signature_validation();

        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .unwrap();

        let ttl = decoded.claims.exp - decoded.claims.iat;
        assert_eq!(ttl, 60, "Internal JWT should have 60s TTL");
    }

    #[test]
    fn test_mint_internal_jwt_preserves_scopes() {
        let mut resolved = make_resolved();
        resolved.scopes = vec!["ledger:read".to_string()];
        let secret = "test-secret";

        let token = mint_internal_jwt(&resolved, secret).unwrap();

        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        let decoded = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation,
        )
        .unwrap();

        assert_eq!(decoded.claims.scopes, vec!["ledger:read"]);
    }

    #[test]
    fn test_claims_serde_scope_rename() {
        let claims = Claims {
            iss: "bankie-gateway".to_string(),
            sub: "test".to_string(),
            aud: "service".to_string(),
            exp: 9999999999,
            iat: 1000000000,
            scopes: vec!["a:b".to_string()],
            tenant_id: 1,
        };

        let json = serde_json::to_string(&claims).unwrap();
        assert!(
            json.contains("\"scope\""),
            "Serialized JSON should use 'scope' not 'scopes'"
        );
    }
}
