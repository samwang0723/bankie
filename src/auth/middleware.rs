use std::{ops::Deref, sync::Arc};

use axum::{body::Body, extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, DecodingKey, TokenData, Validation};
use log::debug;

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
    debug!("Token: {:?}", token);
    let token_data = match decode_jwt(token.unwrap().to_string()) {
        Ok(data) => data,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };
    debug!("Token data: {:?}", token_data.claims);
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
        debug!("Secret key: {:?}", secret_key);
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
