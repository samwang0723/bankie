use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};

use url::Url;

/// Blocked hostname suffixes that resolve to internal services.
const BLOCKED_SUFFIXES: &[&str] = &[".internal", ".local", ".localhost"];

/// Validate a webhook URL hostname against known-bad patterns (no DNS resolution).
///
/// Use at endpoint creation/update time. Catches obvious SSRF targets:
/// localhost, internal suffixes, and literal private IP addresses.
pub fn validate_url_hostname(raw_url: &str) -> Result<(), String> {
    let url = Url::parse(raw_url).map_err(|e| format!("Invalid URL: {e}"))?;

    let host = url
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    // Block localhost directly
    if host == "localhost" {
        return Err("Webhook URLs must not target localhost".to_string());
    }

    // Block known internal suffixes
    let host_lower = host.to_lowercase();
    for suffix in BLOCKED_SUFFIXES {
        if host_lower.ends_with(suffix) {
            return Err(format!(
                "Webhook URLs must not target internal hostnames (*{suffix})"
            ));
        }
    }

    // Block literal private IP addresses in the URL
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Err(format!(
                "Webhook URLs must not target private/reserved IPs ({ip})"
            ));
        }
    }

    // Also check IPv4-mapped forms like http://[::ffff:127.0.0.1]/
    if host.starts_with('[') && host.ends_with(']') {
        if let Ok(ip) = host[1..host.len() - 1].parse::<IpAddr>() {
            if is_private_ip(ip) {
                return Err(format!(
                    "Webhook URLs must not target private/reserved IPs ({ip})"
                ));
            }
        }
    }

    Ok(())
}

/// Full SSRF check with DNS resolution. Use at delivery time (defense in depth).
///
/// Resolves the hostname and validates resolved IPs against private/reserved ranges.
/// Catches DNS rebinding attacks where a hostname initially resolved to a public IP
/// but later resolves to a private IP.
pub fn validate_url_safe(raw_url: &str) -> Result<(), String> {
    // First run hostname-based checks
    validate_url_hostname(raw_url)?;

    let url = Url::parse(raw_url).map_err(|e| format!("Invalid URL: {e}"))?;
    let host = url.host_str().unwrap(); // Safe — validate_url_hostname already checked

    // If host is already a literal IP, we already checked it above
    if host.parse::<Ipv4Addr>().is_ok() || host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    // Resolve hostname to IP and check each address
    let port = url.port().unwrap_or(443);
    let addrs: Vec<_> = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve hostname: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err("Hostname did not resolve to any address".to_string());
    }

    for addr in &addrs {
        if is_private_ip(addr.ip()) {
            return Err(format!(
                "Webhook URLs must not target private/reserved IPs (resolved to {})",
                addr.ip()
            ));
        }
    }

    Ok(())
}

/// Returns true if the IP address is in a private, reserved, or link-local range.
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_loopback()                                        // 127.0.0.0/8
                || octets[0] == 10                                   // 10.0.0.0/8
                || (octets[0] == 172 && (16..=31).contains(&octets[1])) // 172.16.0.0/12
                || (octets[0] == 192 && octets[1] == 168)           // 192.168.0.0/16
                || (octets[0] == 169 && octets[1] == 254)           // 169.254.0.0/16 (IMDS)
                || v4.is_broadcast()                                 // 255.255.255.255
                || v4.is_unspecified() // 0.0.0.0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()                                        // ::1
                || v6.is_unspecified()                               // ::
                || {
                    let segments = v6.segments();
                    (segments[0] & 0xfe00) == 0xfc00                 // fc00::/7 (ULA)
                        || (segments[0] & 0xffc0) == 0xfe80          // fe80::/10 (link-local)
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_private_ip tests ──

    #[test]
    fn test_is_private_ipv4_loopback() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("127.0.0.2".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ipv4_rfc1918() {
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("10.255.255.255".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("172.31.255.255".parse().unwrap()));
        assert!(is_private_ip("192.168.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.255.255".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ipv4_link_local_imds() {
        assert!(is_private_ip("169.254.169.254".parse().unwrap()));
        assert!(is_private_ip("169.254.0.1".parse().unwrap()));
    }

    #[test]
    fn test_is_not_private_ipv4() {
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("203.0.113.1".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ipv6() {
        assert!(is_private_ip("::1".parse().unwrap()));
        assert!(is_private_ip("fc00::1".parse().unwrap()));
        assert!(is_private_ip("fd00::1".parse().unwrap()));
        assert!(is_private_ip("fe80::1".parse().unwrap()));
    }

    #[test]
    fn test_is_not_private_ipv6() {
        assert!(!is_private_ip("2001:db8::1".parse().unwrap()));
        assert!(!is_private_ip("2606:4700::1".parse().unwrap()));
    }

    // ── validate_url_hostname tests (no DNS) ──

    #[test]
    fn test_hostname_localhost_blocked() {
        let result = validate_url_hostname("https://localhost/webhook");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("localhost"));
    }

    #[test]
    fn test_hostname_internal_suffix_blocked() {
        let result = validate_url_hostname("https://service.internal/webhook");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(".internal"));
    }

    #[test]
    fn test_hostname_local_suffix_blocked() {
        let result = validate_url_hostname("https://myapp.local/webhook");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains(".local"));
    }

    #[test]
    fn test_hostname_literal_loopback_blocked() {
        let result = validate_url_hostname("https://127.0.0.1/webhook");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("private"));
    }

    #[test]
    fn test_hostname_literal_private_blocked() {
        assert!(validate_url_hostname("https://10.0.0.1/webhook").is_err());
        assert!(validate_url_hostname("https://172.16.0.1/webhook").is_err());
        assert!(validate_url_hostname("https://192.168.1.1/webhook").is_err());
    }

    #[test]
    fn test_hostname_literal_imds_blocked() {
        assert!(validate_url_hostname("https://169.254.169.254/latest/meta-data/").is_err());
    }

    #[test]
    fn test_hostname_invalid_url() {
        let result = validate_url_hostname("not a url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid URL"));
    }

    #[test]
    fn test_hostname_public_domain_ok() {
        // No DNS resolution — only checks hostname patterns
        assert!(validate_url_hostname("https://api.example.com/hook").is_ok());
        assert!(validate_url_hostname("https://hooks.stripe.com/wh").is_ok());
    }

    // ── validate_url_safe tests (with DNS) ──

    #[test]
    fn test_safe_localhost_blocked() {
        assert!(validate_url_safe("https://localhost/webhook").is_err());
    }

    #[test]
    fn test_safe_loopback_ip_blocked() {
        assert!(validate_url_safe("https://127.0.0.1/webhook").is_err());
    }

    #[test]
    fn test_safe_private_ip_blocked() {
        assert!(validate_url_safe("https://10.0.0.1/webhook").is_err());
    }

    // NOTE: This test makes real DNS resolution.
    #[test]
    fn test_safe_public_url_ok() {
        let result = validate_url_safe("https://example.com/webhook");
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    }
}
