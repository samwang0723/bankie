use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode, Uri},
};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tracing::{error, info};

use crate::config::GatewaySettings;
use crate::middleware::jwt_minter::InternalJwt;

/// Reverse proxy handler that forwards requests to bankie-core.
///
/// Reads `InternalJwt` from extensions (set by jwt_minter middleware),
/// injects it as the Authorization header, and proxies the request.
pub async fn proxy_handler(
    State(settings): State<GatewaySettings>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let internal_jwt = req
        .extensions()
        .get::<InternalJwt>()
        .cloned()
        .ok_or_else(|| {
            error!("InternalJwt not found in extensions — jwt_minter must run first");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let upstream_url = format!("{}{}{}", settings.core_url, path, query);

    info!(method = %method, upstream_url = %upstream_url, "Proxying request to core");

    let upstream_uri: Uri = upstream_url.parse().map_err(|e| {
        error!("Failed to parse upstream URL: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Build upstream request preserving headers (except Authorization and Host)
    let mut builder = Request::builder().method(method).uri(upstream_uri);

    for (name, value) in req.headers() {
        let name_str = name.as_str();
        if name_str == "authorization" || name_str == "host" {
            continue;
        }
        builder = builder.header(name, value);
    }

    // Inject internal JWT
    builder = builder.header("Authorization", format!("Bearer {}", internal_jwt.0));

    let upstream_req = builder.body(req.into_body()).map_err(|e| {
        error!("Failed to build upstream request: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Use hyper client to send the request
    let client = Client::builder(TokioExecutor::new()).build_http();
    let upstream_resp = client.request(upstream_req).await.map_err(|e| {
        error!("Upstream request failed: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    // Convert hyper response to axum response
    let (parts, body) = upstream_resp.into_parts();
    let axum_body = Body::new(body);
    Ok(Response::from_parts(parts, axum_body))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_upstream_url_construction() {
        let base = "http://localhost:3030";
        let path = "/v1/bank_account";
        let query = "?limit=10";
        let url = format!("{}{}{}", base, path, query);
        assert_eq!(url, "http://localhost:3030/v1/bank_account?limit=10");
    }

    #[test]
    fn test_upstream_url_no_query() {
        let base = "http://localhost:3030";
        let path = "/v1/bank_account/123";
        let query = "";
        let url = format!("{}{}{}", base, path, query);
        assert_eq!(url, "http://localhost:3030/v1/bank_account/123");
    }
}
