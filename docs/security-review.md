# Security Review Report — Bankie Developer Portal + Gateway

**Reviewer:** Security Engineer
**Date:** 2026-02-27
**Scope:** Full codebase review — bankie-gateway, bankie-core auth, portal-spa, DB schema, Docker infra
**Gate Decision:** CONDITIONAL PASS

---

## Executive Summary

The codebase demonstrates solid security fundamentals — argon2id password hashing, CSRF protection via double-submit pattern, SHA-256 API key hashing, short-lived internal JWTs (60s), tenant isolation, scope-based access control, and parameterized SQL queries. However, several findings across HIGH and MEDIUM severity require attention before production deployment.

---

## Findings by Severity

### CRITICAL — None

### HIGH (3 findings)

**H1: Session cookie missing `Secure` flag**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:187`
- **Issue:** `portal_session` cookie is set with `HttpOnly; SameSite=Lax` but missing `Secure` flag. In production over HTTPS, this cookie can still be sent over HTTP if someone visits an HTTP URL, enabling session hijacking via network sniffing.
- **Fix:** Add `Secure` flag: `portal_session={jwt}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=86400`. Consider making this conditional on `ENV != local`.
- **STRIDE:** Information Disclosure

**H2: JWT token exposed in login/signup response body**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:170` (`AuthResponse.token`)
- **Issue:** The raw JWT token is returned in the JSON response body alongside the httpOnly session cookie. The SPA stores this in `localStorage` (`portal-spa/src/hooks/useAuth.ts:60`). localStorage is accessible to any JS running on the page — if there's ever an XSS, the attacker gets the full session JWT. The httpOnly cookie pattern was implemented correctly to prevent this, but the token body field undermines it.
- **Fix:** Remove `token` from the `AuthResponse` JSON body. The session cookie is sufficient for auth. If the SPA needs to check "am I logged in", use a `/auth/me` endpoint or check for the CSRF cookie presence instead.
- **STRIDE:** Information Disclosure, Elevation of Privilege

**H3: `create_org` endpoint lacks RBAC — any authenticated user can create orgs with arbitrary `tenant_id`**
- **File:** `crates/bankie-gateway/src/routes/org.rs:25-55`
- **Issue:** The `POST /portal/v1/orgs` handler accepts `tenant_id` from the request body (`CreateOrgRequest.tenant_id`). Any authenticated portal user can create organizations and specify any `tenant_id`, potentially gaining access to another org's banking data. This is a tenant isolation bypass.
- **Fix:** Either (a) ignore `tenant_id` from the request body and use the DB sequence (`DEFAULT nextval`), or (b) restrict org creation to a super-admin role. The DB schema already auto-assigns `tenant_id` via sequence, so the request body field should be removed.
- **STRIDE:** Elevation of Privilege, Spoofing

---

### MEDIUM (5 findings)

**M1: No brute-force protection on login endpoint**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:86-112`
- **Issue:** The login endpoint has no rate limiting. An attacker can make unlimited password guessing attempts. While argon2id is slow, it still only provides ~1s per attempt, so a focused attack on a known email is practical.
- **Fix:** Add per-IP and per-email rate limiting on `/auth/login` and `/auth/signup` — e.g., max 5 failed attempts per email per 15 minutes, max 20 login attempts per IP per minute.
- **STRIDE:** Spoofing, Elevation of Privilege

**M2: Docker Compose — PostgreSQL and Redis with empty passwords**
- **File:** `docker-compose.yml:10,36`
- **Issue:** Both PostgreSQL and Redis run with `ALLOW_EMPTY_PASSWORD=yes`. While acceptable for local dev, this file is often copied to staging/production. The default `DB_PASSWD` fallback is `localpass` (line 57,100,125).
- **Fix:** Remove `ALLOW_EMPTY_PASSWORD=yes`. Require explicit password via env vars. Add a comment warning against using defaults in production.
- **STRIDE:** Spoofing, Information Disclosure

**M3: Rate limiter fails open on Redis unavailability**
- **File:** `crates/bankie-gateway/src/middleware/rate_limiter.rs:39-62,87`
- **Issue:** When Redis is down, rate limiting is completely bypassed (`None => Ok(next.run(req).await)`). An attacker can DoS the system or bypass rate limits by overloading Redis first.
- **Fix:** Consider a local in-memory fallback rate limiter (e.g., `governor` crate) that kicks in when Redis is unavailable. At minimum, log at `WARN` level (already done) and consider a circuit breaker pattern.
- **STRIDE:** Denial of Service

**M4: API key resolution cached without revocation invalidation**
- **File:** `crates/bankie-gateway/src/middleware/api_key_resolver.rs:13-16`
- **Issue:** Resolved API keys are cached in Redis for 5 minutes (`CACHE_TTL_SECS = 300`). When a key is revoked via the portal, it remains valid in the cache for up to 5 minutes. For a banking system, this window is too long.
- **Fix:** On revoke/rotate, actively delete the cache entry `gw:api_key:{key_hash}` from Redis. Alternatively, reduce TTL to 30-60 seconds.
- **STRIDE:** Elevation of Privilege

**M5: `build_query` does not URL-encode parameter values**
- **File:** `crates/bankie-gateway/src/routes/data_proxy.rs:135-145`
- **Issue:** Query parameter values are concatenated directly into the URL without URL-encoding. While most values come from typed query params (numbers, dates), user-provided fields like `bank_account_id` or `currency` could contain special characters that break the URL or enable query manipulation against the upstream service.
- **Fix:** Use `urlencoding::encode()` on values before concatenation, or use a proper URL builder.
- **STRIDE:** Tampering

---

### LOW (4 findings)

**L1: Password policy too permissive — only checks length >= 8**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:37-41`
- **Issue:** No complexity requirements. Passwords like "password" or "12345678" are accepted.
- **Fix:** Add basic complexity checks (at least 1 uppercase, 1 lowercase, 1 digit) or use a password strength library like `zxcvbn`.

**L2: Logout doesn't invalidate session server-side**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:117-130`
- **Issue:** Logout only clears cookies client-side. The JWT remains valid until expiry (24h). A stolen JWT continues to work after logout.
- **Fix:** Implement a token blocklist in Redis (key: JWT jti/hash, TTL: remaining token lifetime). Check blocklist in `session_auth` middleware.

**L3: CSRF token cookie missing `Secure` flag**
- **File:** `crates/bankie-gateway/src/routes/auth.rs:188`
- **Issue:** The `csrf_token` cookie is not `HttpOnly`, which is by design (SPA needs to read it). However, it's also not marked `Secure`, meaning it can leak over HTTP in production.
- **Fix:** Add `Secure` flag to the CSRF cookie in production environments.

**L4: Core JWT issued with 365-day expiry**
- **File:** `crates/bankie-core/src/auth/jwt.rs:39-42`
- **Issue:** The `generate_jwt` function creates JWTs that expire in 365 days. If a token leaks, it's valid for a year. The gateway's internal JWTs (60s) are properly short-lived, but the core tenant tokens are not.
- **Fix:** Reduce to 30-90 days with a refresh mechanism, or since the gateway now handles auth, deprecate direct core JWT issuance.

---

## Threat Model (STRIDE)

| Threat | Coverage | Notes |
|--------|----------|-------|
| **Spoofing** | Good | Argon2id, JWT validation, API key SHA-256 hashing |
| **Tampering** | Good | CSRF double-submit, tenant_id injected server-side (core), parameterized SQL |
| **Repudiation** | Partial | Audit logs table exists but no code writes to it yet |
| **Info Disclosure** | Needs work | H1 (Secure flag), H2 (token in body), key_hash `#[serde(skip)]` is good |
| **DoS** | Partial | Rate limiting exists but fails open (M3), no login rate limit (M1) |
| **Elevation** | Needs work | H3 (org creation), M4 (cache invalidation gap) |

---

## Compliance Notes (SOC2 / PCI-DSS relevant)

- **Encryption at rest:** Not assessed (infrastructure dependent)
- **Encryption in transit:** Cookies missing `Secure` flag (H1, L3)
- **Password storage:** Argon2id — excellent choice
- **Audit logging:** Schema exists (`portal.audit_logs`) but not populated by application code yet
- **Session management:** 24h sessions, no server-side invalidation (L2)
- **Access control:** RBAC implemented for org updates, but org creation is open (H3)

---

## Gate Decision: CONDITIONAL PASS

**Must fix before production:**
1. **H1** — Add `Secure` flag to session cookie
2. **H2** — Remove JWT token from response body
3. **H3** — Remove `tenant_id` from `CreateOrgRequest` or restrict org creation

**Should fix before production (strongly recommended):**
4. **M1** — Add login rate limiting
5. **M4** — Invalidate API key cache on revoke/rotate

**Can fix post-launch:**
6. M2, M3, M5, L1-L4
