use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use sqlx::PgPool;
use tracing::warn;

use crate::repo::api_key::ResolvedApiKey;

struct ApiLogEntry {
    api_key_id: uuid::Uuid,
    tenant_id: i32,
    method: String,
    path: String,
    status_code: i32,
    latency_ms: i32,
    client_ip: Option<String>,
}

/// Middleware that logs API proxy requests to portal.api_logs.
///
/// Records method, path, status_code, latency, and client IP for each
/// proxied request. Requires `PgPool` and `ResolvedApiKey` in extensions.
/// Logging failures are best-effort (warned, not propagated).
pub async fn api_logger(req: Request<Body>, next: Next) -> Response<Body> {
    let pool = req.extensions().get::<PgPool>().cloned();
    let resolved_key = req.extensions().get::<ResolvedApiKey>().cloned();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();
    let client_ip = extract_client_ip(&req);
    let start = std::time::Instant::now();

    let response = next.run(req).await;

    let status_code = response.status().as_u16() as i32;
    let latency_ms = start.elapsed().as_millis() as i32;

    // Best-effort async log insert
    if let (Some(pool), Some(key)) = (pool, resolved_key) {
        let entry = ApiLogEntry {
            api_key_id: key.api_key_id,
            tenant_id: key.tenant_id,
            method,
            path,
            status_code,
            latency_ms,
            client_ip,
        };
        tokio::spawn(async move {
            if let Err(e) = insert_api_log(&pool, &entry).await {
                warn!("Failed to insert API log: {}", e);
            }
        });
    }

    response
}

fn extract_client_ip(req: &Request<Body>) -> Option<String> {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
}

async fn insert_api_log(pool: &PgPool, entry: &ApiLogEntry) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO portal.api_logs (api_key_id, tenant_id, method, path, status_code, latency_ms, client_ip, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, now())
        "#,
    )
    .bind(entry.api_key_id)
    .bind(entry.tenant_id)
    .bind(&entry.method)
    .bind(&entry.path)
    .bind(entry.status_code)
    .bind(entry.latency_ms)
    .bind(&entry.client_ip)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::HeaderValue;

    fn build_request_with_header(name: &str, value: &str) -> Request<Body> {
        Request::builder()
            .header(name, HeaderValue::from_str(value).unwrap())
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn test_extract_client_ip_from_x_forwarded_for() {
        let req = build_request_with_header("x-forwarded-for", "1.2.3.4, 5.6.7.8");
        assert_eq!(extract_client_ip(&req), Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_extract_client_ip_from_x_real_ip() {
        let req = build_request_with_header("x-real-ip", "10.0.0.1");
        assert_eq!(extract_client_ip(&req), Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_extract_client_ip_missing() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert_eq!(extract_client_ip(&req), None);
    }

    #[test]
    fn test_extract_client_ip_prefers_x_forwarded_for() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .body(Body::empty())
            .unwrap();
        assert_eq!(extract_client_ip(&req), Some("1.2.3.4".to_string()));
    }
}
