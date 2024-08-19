use std::sync::Arc;

use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, DecodingKey, TokenData, Validation};
use tracing::debug;

use crate::{repository::adapter::DatabaseClient, state::ApplicationState};

use super::jwt::Claims;

pub async fn authorize<C: DatabaseClient + Send + Sync + 'static>(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, StatusCode> {
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

    let state = match req.extensions().get::<Arc<ApplicationState<C>>>() {
        Some(state) => state,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    match state
        .database
        .get_tenant_profile(token_data.claims.tenant_id)
        .await
    {
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
    use crate::{
        domain::tenant::Tenant,
        repository::adapter::{Adapter, MockDatabaseClient},
    };

    use super::*;
    use axum::{middleware, Router};
    use chrono::{Duration, Utc};
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::env;
    use tower::ServiceExt;

    impl Clone for MockDatabaseClient {
        fn clone(&self) -> Self {
            MockDatabaseClient::new()
        }
    }

    #[tokio::test]
    async fn test_authorize() {
        env::set_var("JWT_SECRET", "your_secret_key");
        let expiration = Utc::now()
            .checked_add_signed(Duration::days(365))
            .expect("valid timestamp")
            .timestamp();

        let claims = Claims {
            sub: "test_service".to_string(),
            exp: expiration as usize,
            iat: Utc::now().timestamp() as usize,
            iss: "bankie".to_owned(),
            aud: "service".to_owned(),
            scopes: vec![
                "bank-account:read".to_owned(),
                "bank-account:write".to_owned(),
                "ledger:read".to_owned(),
            ],
            tenant_id: 1,
        };

        let header = Header::default();
        let encoding_key = EncodingKey::from_secret("your_secret_key".as_bytes());
        let token = encode(&header, &claims, &encoding_key).unwrap();

        let mut mock_db_client = MockDatabaseClient::new();
        let tenant = Tenant {
            id: 1,
            name: "test_service".to_string(),
            jwt: token.clone(),
            scope: Some("bank-account:read bank-account:write ledger:read".to_string()),
            status: "active".to_string(),
        };
        mock_db_client
            .expect_get_tenant_profile()
            .returning(move |_| Ok(tenant.clone()));
        let state = ApplicationState::<MockDatabaseClient>::new(Adapter::new(mock_db_client));

        // Create a request with the authorization header
        let mut req = Request::builder()
            .header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {}", token),
            )
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(state.clone());

        let app = Router::new()
            .route("/", axum::routing::get(|| async { "test" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                authorize::<MockDatabaseClient>,
            ))
            .with_state(state);

        let response = app.clone().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_authorize_missing_header() {
        // Create a mock request without the Authorization header
        let req = Request::builder().body(Body::empty()).unwrap();
        let app = Router::new()
            .route("/", axum::routing::post(|| async { "test" }))
            .layer(middleware::from_fn(authorize::<MockDatabaseClient>));
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_authorize_wrong_jwt() {
        let mut mock_db_client = MockDatabaseClient::new();
        let tenant = Tenant {
            id: 1,
            name: "test_service".to_string(),
            jwt: "correct_token".to_string(),
            scope: Some("bank-account:read bank-account:write ledger:read".to_string()),
            status: "active".to_string(),
        };
        mock_db_client
            .expect_get_tenant_profile()
            .returning(move |_| Ok(tenant.clone()));
        let state = ApplicationState::<MockDatabaseClient>::new(Adapter::new(mock_db_client));

        // Create a request with the authorization header
        let mut req = Request::builder()
            .header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {}", "wrong_token"),
            )
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(state.clone());

        let app = Router::new()
            .route("/", axum::routing::get(|| async { "test" }))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                authorize::<MockDatabaseClient>,
            ))
            .with_state(state);

        let response = app.clone().oneshot(req).await.unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_decode_jwt() {
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
        env::set_var("JWT_SECRET", "your_secret_key");
        let encoding_key = EncodingKey::from_secret("your_secret_key".as_bytes());
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
