use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};
use sqlx::PgPool;
use tracing::warn;

use crate::repo::api_key::ResolvedApiKey;

/// Sensitive query parameter names that should be stripped from logged paths.
const SENSITIVE_PARAMS: &[&str] = &[
    "token",
    "secret",
    "password",
    "key",
    "api_key",
    "credential",
];

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
/// proxied request. PII is redacted before storage:
/// - IP addresses are masked to /24 (IPv4) or /48 (IPv6)
/// - Sensitive query params (token, secret, password, key) are stripped
///
/// Requires `PgPool` and `ResolvedApiKey` in extensions.
/// Logging failures are best-effort (warned, not propagated).
pub async fn api_logger(req: Request<Body>, next: Next) -> Response<Body> {
    let pool = req.extensions().get::<PgPool>().cloned();
    let resolved_key = req.extensions().get::<ResolvedApiKey>().cloned();
    let method = req.method().to_string();
    let raw_path = req
        .uri()
        .path_and_query()
        .map_or_else(|| req.uri().path().to_string(), |pq| pq.to_string());
    let client_ip = extract_client_ip(&req);
    let start = std::time::Instant::now();

    let response = next.run(req).await;

    let status_code = response.status().as_u16() as i32;
    let latency_ms = start.elapsed().as_millis() as i32;

    // Best-effort async log insert with PII redaction
    if let (Some(pool), Some(key)) = (pool, resolved_key) {
        let entry = ApiLogEntry {
            api_key_id: key.api_key_id,
            tenant_id: key.tenant_id,
            method,
            path: redact_path(&raw_path),
            status_code,
            latency_ms,
            client_ip: client_ip.as_deref().map(redact_ip),
        };
        tokio::spawn(async move {
            if let Err(e) = insert_api_log(&pool, &entry).await {
                warn!("Failed to insert API log: {}", e);
            }
        });
    }

    response
}

/// Redact an IP address to its subnet.
///
/// IPv4: masks to /24 (e.g., 192.168.1.42 → 192.168.1.0)
/// IPv6: masks to /48 (e.g., 2001:db8:85a3::1 → 2001:db8:85a3::)
/// Non-parseable IPs are returned as `***`.
pub fn redact_ip(ip: &str) -> String {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            let octets = v4.octets();
            Ipv4Addr::new(octets[0], octets[1], octets[2], 0).to_string()
        }
        Ok(IpAddr::V6(v6)) => {
            let segs = v6.segments();
            // Keep first 3 segments (/48), zero out the rest
            Ipv6Addr::new(segs[0], segs[1], segs[2], 0, 0, 0, 0, 0).to_string()
        }
        Err(_) => "***".to_string(),
    }
}

/// Redact sensitive query parameters from a path.
///
/// Strips values of params named token, secret, password, key, api_key,
/// credential (case-insensitive) and replaces them with `[REDACTED]`.
pub fn redact_path(path: &str) -> String {
    let Some(query_start) = path.find('?') else {
        return path.to_string();
    };

    let base = &path[..query_start];
    let query = &path[query_start + 1..];

    let redacted_params: Vec<String> = query
        .split('&')
        .map(|param| {
            if let Some(eq_pos) = param.find('=') {
                let name = &param[..eq_pos];
                if SENSITIVE_PARAMS
                    .iter()
                    .any(|s| name.eq_ignore_ascii_case(s))
                {
                    return format!("{}=[REDACTED]", name);
                }
            }
            param.to_string()
        })
        .collect();

    if redacted_params.is_empty() {
        base.to_string()
    } else {
        format!("{}?{}", base, redacted_params.join("&"))
    }
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

    // === extract_client_ip tests ===

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

    // === redact_ip tests ===

    #[test]
    fn test_redact_ipv4_masks_to_24() {
        assert_eq!(redact_ip("192.168.1.42"), "192.168.1.0");
        assert_eq!(redact_ip("10.0.255.123"), "10.0.255.0");
        assert_eq!(redact_ip("1.2.3.4"), "1.2.3.0");
    }

    #[test]
    fn test_redact_ipv6_masks_to_48() {
        let result = redact_ip("2001:db8:85a3::8a2e:370:7334");
        assert_eq!(result, "2001:db8:85a3::");
    }

    #[test]
    fn test_redact_ip_invalid() {
        assert_eq!(redact_ip("not-an-ip"), "***");
        assert_eq!(redact_ip(""), "***");
    }

    #[test]
    fn test_redact_ipv4_localhost() {
        assert_eq!(redact_ip("127.0.0.1"), "127.0.0.0");
    }

    // === redact_path tests ===

    #[test]
    fn test_redact_path_no_query() {
        assert_eq!(redact_path("/v1/accounts"), "/v1/accounts");
    }

    #[test]
    fn test_redact_path_no_sensitive_params() {
        assert_eq!(
            redact_path("/v1/accounts?offset=0&limit=10"),
            "/v1/accounts?offset=0&limit=10"
        );
    }

    #[test]
    fn test_redact_path_strips_token() {
        assert_eq!(
            redact_path("/v1/auth?token=abc123&mode=test"),
            "/v1/auth?token=[REDACTED]&mode=test"
        );
    }

    #[test]
    fn test_redact_path_strips_secret() {
        assert_eq!(
            redact_path("/callback?secret=mysecret"),
            "/callback?secret=[REDACTED]"
        );
    }

    #[test]
    fn test_redact_path_strips_password() {
        assert_eq!(
            redact_path("/login?password=hunter2&user=sam"),
            "/login?password=[REDACTED]&user=sam"
        );
    }

    #[test]
    fn test_redact_path_strips_key() {
        assert_eq!(redact_path("/api?key=bk_live_abc"), "/api?key=[REDACTED]");
    }

    #[test]
    fn test_redact_path_strips_api_key() {
        assert_eq!(
            redact_path("/v1/data?api_key=sk_test_123"),
            "/v1/data?api_key=[REDACTED]"
        );
    }

    #[test]
    fn test_redact_path_case_insensitive() {
        assert_eq!(
            redact_path("/api?TOKEN=abc&Secret=xyz"),
            "/api?TOKEN=[REDACTED]&Secret=[REDACTED]"
        );
    }

    #[test]
    fn test_redact_path_multiple_sensitive() {
        assert_eq!(
            redact_path("/api?token=a&key=b&password=c&normal=d"),
            "/api?token=[REDACTED]&key=[REDACTED]&password=[REDACTED]&normal=d"
        );
    }

    #[test]
    fn test_redact_path_credential_param() {
        assert_eq!(
            redact_path("/api?credential=secret_val"),
            "/api?credential=[REDACTED]"
        );
    }

    #[test]
    fn test_sensitive_params_list() {
        assert_eq!(SENSITIVE_PARAMS.len(), 6);
        assert!(SENSITIVE_PARAMS.contains(&"token"));
        assert!(SENSITIVE_PARAMS.contains(&"secret"));
        assert!(SENSITIVE_PARAMS.contains(&"password"));
        assert!(SENSITIVE_PARAMS.contains(&"key"));
        assert!(SENSITIVE_PARAMS.contains(&"api_key"));
        assert!(SENSITIVE_PARAMS.contains(&"credential"));
    }
}
