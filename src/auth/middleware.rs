use std::{ops::Deref, sync::Arc};

use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, DecodingKey, TokenData, Validation};
use tracing::debug;

use crate::{auth::tenant::get_tenant_profile, state::ApplicationState};

use super::jwt::Claims;

pub async fn authorize(mut req: Request, next: Next) -> Result<Response<Body>, StatusCode> {
    let auth_header = req.headers_mut().get(axum::http::header::AUTHORIZATION);
    let auth_header = match auth_header {
        Some(header) => header.to_str().map_err(|_| StatusCode::FORBIDDEN)?,
        None => return Err(StatusCode::FORBIDDEN),
    };
    let mut header = auth_header.split_whitespace();
    let (_bearer, token) = (header.next(), header.next());
    let token_data = match decode_jwt(token.unwrap().to_string()) {
        Ok(data) => data,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };
    let state = req.extensions().get::<Arc<ApplicationState>>().unwrap();
    match get_tenant_profile(Arc::clone(&state.pool).deref(), token_data.claims.tenant_id).await {
        Ok(tenant) => {
            debug!("Tenant: {:?}", tenant);
            req.extensions_mut().insert(tenant.id);
            Ok(next.run(req).await)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

pub fn decode_jwt(jwt_token: String) -> Result<TokenData<Claims>, StatusCode> {
    if let Ok(secret_key) = std::env::var("JWT_SECRET") {
        let mut validation = Validation::default();
        validation.set_audience(&["service"]);
        let result: Result<TokenData<Claims>, StatusCode> = decode(
            &jwt_token,
            &DecodingKey::from_secret(secret_key.as_ref()),
            &validation,
        )
        .map_err(|e| {
            debug!("Error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        });
        result
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::jwt::generate_secret_key;

    use super::*;
    use chrono::Utc;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::env;

    #[tokio::test]
    async fn test_decode_jwt() {
        // Set up the environment variable for the secret key
        let secret_key = generate_secret_key(32);
        env::set_var("JWT_SECRET", &secret_key);

        // Generate a JWT token
        let claims = Claims {
            iss: "bankie".to_owned(),
            sub: "test_service".to_owned(),
            aud: "service".to_owned(),
            exp: (Utc::now().timestamp() + 3600) as usize, // 1 hour expiration
            iat: Utc::now().timestamp() as usize,
            scopes: vec![
                "bank-account:read".to_owned(),
                "bank-account:write".to_owned(),
                "ledger:read".to_owned(),
            ],
            tenant_id: 1,
        };

        let header = Header::default();
        let encoding_key = EncodingKey::from_secret(secret_key.as_bytes());
        let jwt_token = encode(&header, &claims, &encoding_key).unwrap();

        // Decode the JWT token
        let result = decode_jwt(jwt_token);

        // Validate the result
        assert!(result.is_ok(), "JWT decoding failed");
        let token_data = result.unwrap();
        let decoded_claims = token_data.claims;

        assert_eq!(decoded_claims.iss, claims.iss, "Issuer mismatch");
        assert_eq!(decoded_claims.sub, claims.sub, "Subject mismatch");
        assert_eq!(decoded_claims.aud, claims.aud, "Audience mismatch");
        assert_eq!(decoded_claims.exp, claims.exp, "Expiration time mismatch");
        assert_eq!(decoded_claims.iat, claims.iat, "Issued at time mismatch");
        assert_eq!(decoded_claims.scopes, claims.scopes, "Scopes mismatch");
        assert_eq!(
            decoded_claims.tenant_id, claims.tenant_id,
            "Tenant ID mismatch"
        );
    }
}
