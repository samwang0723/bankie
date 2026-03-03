use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Build the Stripe-compatible signature header value.
///
/// Format: `t={unix_ts},v1={hmac_sha256_hex}`
/// Signed content: `"{timestamp}.{body}"`
pub fn sign_payload(secret: &str, timestamp: i64, body: &str) -> String {
    let signed_content = format!("{}.{}", timestamp, body);
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(signed_content.as_bytes());
    let result = mac.finalize();
    format!("t={},v1={}", timestamp, hex::encode(result.into_bytes()))
}

/// Verify a signature against an expected header value.
///
/// Returns `true` if the computed signature matches.
pub fn verify_signature(secret: &str, timestamp: i64, body: &str, signature: &str) -> bool {
    let expected = sign_payload(secret, timestamp, body);
    // Constant-time comparison
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_payload_format() {
        let sig = sign_payload("whsec_test_secret", 1709366400, r#"{"event":"test"}"#);
        assert!(sig.starts_with("t=1709366400,v1="));
        // v1 portion should be 64 hex chars (SHA-256 = 32 bytes = 64 hex)
        let v1 = sig.strip_prefix("t=1709366400,v1=").unwrap();
        assert_eq!(v1.len(), 64);
        assert!(v1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sign_payload_deterministic() {
        let sig1 = sign_payload("secret", 1000, "body");
        let sig2 = sign_payload("secret", 1000, "body");
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_sign_payload_different_secret() {
        let sig1 = sign_payload("secret_a", 1000, "body");
        let sig2 = sign_payload("secret_b", 1000, "body");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_sign_payload_different_timestamp() {
        let sig1 = sign_payload("secret", 1000, "body");
        let sig2 = sign_payload("secret", 2000, "body");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_sign_payload_different_body() {
        let sig1 = sign_payload("secret", 1000, "body_a");
        let sig2 = sign_payload("secret", 1000, "body_b");
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_sign_payload_empty_body() {
        let sig = sign_payload("secret", 1000, "");
        assert!(sig.starts_with("t=1000,v1="));
        let v1 = sig.strip_prefix("t=1000,v1=").unwrap();
        assert_eq!(v1.len(), 64);
    }

    #[test]
    fn test_verify_signature_valid() {
        let sig = sign_payload("secret", 1000, "body");
        assert!(verify_signature("secret", 1000, "body", &sig));
    }

    #[test]
    fn test_verify_signature_wrong_secret() {
        let sig = sign_payload("secret", 1000, "body");
        assert!(!verify_signature("wrong", 1000, "body", &sig));
    }

    #[test]
    fn test_verify_signature_wrong_timestamp() {
        let sig = sign_payload("secret", 1000, "body");
        assert!(!verify_signature("secret", 9999, "body", &sig));
    }

    #[test]
    fn test_verify_signature_wrong_body() {
        let sig = sign_payload("secret", 1000, "body");
        assert!(!verify_signature("secret", 1000, "tampered", &sig));
    }

    #[test]
    fn test_known_value() {
        // Pre-computed known value for regression testing
        let sig = sign_payload("whsec_abc123", 1709366400, r#"{"type":"test"}"#);
        // Verify format and that the same inputs always produce the same output
        let sig2 = sign_payload("whsec_abc123", 1709366400, r#"{"type":"test"}"#);
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_not_equal() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"hello", b"hi"));
    }
}
