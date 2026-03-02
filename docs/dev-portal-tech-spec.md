# Tech Spec: Bankie Developer Portal

| Field | Value |
|-------|-------|
| **Owner Pillar** | Fiat / Onboarding |
| **Feature Label** | Developer Portal |
| **Authors** | sam.wang (SVP Eng), Architect Agent |
| **Audiences** | Engineering, Product, Security, Compliance |
| **Status** | Draft |
| **Version** | 2.0 |
| **Reviewers** | — |
| **Useful Links** | PRD: `.claude/tasks/dev-portal-prd/pm.md` · Architecture: `.claude/tasks/dev-portal-prd/architect.md` |
| **Approved Date** | — |

---

## 1. TL;DR Change Summary

The Bankie Developer Portal introduces a **self-service API Gateway** (`bankie-gateway`) and **React SPA dashboard** (`portal-spa`) that wrap Bankie Core without modifying existing code. Tenants authenticate via API keys (`bk_live_*`) instead of manually provisioned JWTs. The Gateway resolves keys, enforces scoped permissions, applies rate limiting, mints short-lived internal JWTs, and reverse-proxies requests to Core. Portal members manage organizations, API keys, and view banking data through a session-authenticated dashboard with CSRF protection.

An **FX Rate Engine** in bankie-core fetches live exchange rates from CoinGecko (crypto) and ExchangeRate API (fiat), caches them in Redis, and records a USD-normalized amount (`amount_usd`) on every transaction for cross-currency accounting and reporting.

**Impact**: 90% reduction in tenant onboarding time. Zero manual JWT provisioning. Scoped API access with key rotation and grace periods. USD-normalized cross-currency aggregation for dashboard and settlement reports.

---

## 2. Background

### 2.1 Objective

Bankie Core is a production-grade CQRS/Event Sourcing banking system, but tenant onboarding requires manual CLI-based JWT generation (`cargo run --bin bankie -- --mode jwt`). There is no self-service key management, no scoped permissions, no rate limiting, and no team collaboration. The Developer Portal solves these gaps following the industry pattern established by Stripe, Column, Increase, and Modern Treasury.

### 2.2 Goals

| Priority | Goal |
|----------|------|
| P0 | Self-service org signup → API key generation in < 5 minutes |
| P0 | Secure API key lifecycle: create, rotate (with grace period), revoke, scope enforcement |
| P0 | API Gateway with rate limiting (100 req/s burst, 1000/min sustained) |
| P0 | Session-based portal auth with CSRF protection |
| P0 | FX Rate Engine — USD normalization on every transaction for cross-currency accounting |
| P1 | Data proxy for portal SPA to read Core banking data (accounts, transactions, reports) |
| P1 | Dashboard with real-time statistics: API call counts (24h), scopes granted, audit activity feed |
| P1 | Login brute-force protection (5 attempts / 15-min lockout) with 429 + Retry-After |
| P1 | API request logging middleware with async insert to `portal.api_logs` |
| P1 | Audit logging on key operations (create, rotate, revoke) to `portal.audit_logs` |
| P1 | API key cache invalidation on revoke/rotate (Redis DEL) |
| P2 | Webhook delivery with HMAC-SHA256 signing (schema defined, not yet implemented) |

### 2.3 Non-Goals

- Replacing the CQRS/Event Sourcing core architecture
- Mobile app for the portal
- Multi-region deployment (v1 is single-region)
- Custom branding / white-label portal per tenant
- Billing / usage-based pricing
- GraphQL API
- OAuth2 / OIDC for portal login (password-based auth for v1)

---

## 3. Architecture Overview

### 3.1 System Architecture

```mermaid
graph TB
    subgraph "External"
        DEV[Developer / Integration]
        BROWSER[Browser / Dashboard]
    end

    subgraph "FX Rate Sources"
        CG[CoinGecko API<br/>BTC, ETH, USDT rates]
        ER[ExchangeRate API<br/>TWD rate]
    end

    subgraph "Developer Portal Layer"
        subgraph "Portal SPA :8080"
            REACT[React + Vite + TypeScript<br/>TailwindCSS + React Router<br/>React Query]
        end

        subgraph "bankie-gateway :4040"
            subgraph "Portal API /portal/v1/*"
                AUTH_R[Auth Routes<br/>signup / login / logout<br/>brute-force protection]
                ORG_R[Org CRUD]
                KEY_R[API Key CRUD<br/>create / list / rotate / revoke<br/>+ audit log + cache invalidation]
                DASH_R[Dashboard Stats + Activity Feed<br/>real API call counts + audit trail]
                DATA_R[Data Proxy<br/>accounts / transactions / reports]
            end

            subgraph "Gateway Middleware /v1/*"
                AKR[API Key Resolver<br/>SHA-256 + Redis cache 5min]
                RL[Rate Limiter<br/>Redis token bucket]
                ALOG[API Logger<br/>async insert to api_logs]
                MINT[JWT Minter<br/>60s internal JWT]
                PROXY[Reverse Proxy<br/>hyper-util]
            end

            subgraph "Portal Auth"
                SESSION[Session Middleware<br/>Cookie JWT + CSRF<br/>Secure flag conditional on ENV]
                SCOPE[Scope Enforcer<br/>route → scope mapping]
            end
        end
    end

    subgraph "Bankie Core :3030"
        CORE_AUTH[JWT Auth Middleware]
        ROUTES[/v1/* Route Handlers]
        CMD[Command Channel mpsc 10k]
        CQRS[CQRS/ES Aggregates]
        FX[FxRateService<br/>Redis cache 60s/1h<br/>graceful degradation]
    end

    subgraph "Infrastructure"
        PG[(PostgreSQL 16<br/>public + portal schemas)]
        RD[(Redis<br/>cache + rate limit + FX rates)]
    end

    DEV -->|"Bearer bk_live_xxx"| AKR
    BROWSER -->|HTTPS| REACT
    REACT -->|"Cookie + CSRF"| AUTH_R & ORG_R & KEY_R & DASH_R & DATA_R

    AKR --> RL --> ALOG --> MINT --> PROXY
    PROXY -->|"Bearer internal-jwt"| CORE_AUTH
    CORE_AUTH --> ROUTES --> CMD --> CQRS

    DATA_R -->|"minted JWT"| CORE_AUTH

    CQRS -->|convert_to_usd| FX
    FX --> RD
    FX --> CG
    FX --> ER

    ORG_R & KEY_R --> PG
    AKR --> RD
    RL --> RD
```

### 3.2 Cargo Workspace Structure

```
bankie/
├── Cargo.toml                    # [workspace] members, shared deps
├── crates/
│   ├── bankie-core/              # CQRS/ES banking system + FX Rate Engine
│   │   └── src/
│   │       ├── common/
│   │       │   ├── fx_rate.rs    # FxRateService, FxRateProvider trait, CoinGecko/ExchangeRate providers
│   │       │   ├── money.rs      # Money type, Currency enum, precision rules
│   │       │   └── asset.rs      # AssetRegistry (runtime)
│   │       ├── domain/finance.rs # Transaction + 3 FX fields, SettlementReportRow + FX columns
│   │       ├── report.rs         # Settlement CSV generator (15 columns incl. FX)
│   │       └── ...               # (42+ .rs files)
│   ├── bankie-gateway/           # API Gateway + Portal Management API
│   │   └── src/
│   │       ├── main.rs           # Gateway binary — Axum server on :4040
│   │       ├── config.rs         # GatewaySettings (lazy_static, config crate)
│   │       ├── state.rs          # PortalState (trait-object repos + jwt_secret)
│   │       ├── proxy.rs          # Reverse proxy to Core via hyper-util
│   │       ├── redis_ops.rs      # Redis helpers (get, set_ex, del, incr_with_expiry, get_ttl)
│   │       ├── middleware/
│   │       │   ├── api_key_resolver.rs  # SHA-256 lookup + Redis cache
│   │       │   ├── api_logger.rs        # Async API request logging to portal.api_logs
│   │       │   ├── rate_limiter.rs      # Token bucket + X-RateLimit-* headers
│   │       │   ├── jwt_minter.rs        # 60s internal JWT for Core
│   │       │   ├── scope_enforcer.rs    # Route → scope mapping + enforcement
│   │       │   └── session.rs           # Cookie JWT + CSRF + conditional Secure flag
│   │       ├── routes/
│   │       │   ├── auth.rs       # signup / login / logout + brute-force rate limiting
│   │       │   ├── org.rs        # org CRUD
│   │       │   ├── api_key.rs    # key CRUD + audit logging + cache invalidation
│   │       │   ├── dashboard.rs  # stats + activity feed (real data from api_logs + audit_logs)
│   │       │   └── data_proxy.rs # proxy Core read endpoints for SPA
│   │       ├── models/           # Domain models (auth, org, member, api_key)
│   │       └── repo/             # Repository traits + PgPool impls + mocks
│   └── bankie-common/            # Shared types
│       └── src/
│           ├── lib.rs
│           └── error.rs          # AppError enum (400-500 + 429 TooManyRequests)
├── portal-spa/                   # React SPA dashboard
│   └── src/
│       ├── App.tsx               # Routes: Dashboard, ApiKeys, Org, Accounts, etc.
│       ├── hooks/useAuth.ts      # Auth context + session management
│       ├── api/client.ts         # Fetch wrapper with cookie auth
│       ├── utils/currency.ts     # Shared currency formatting (formatUsdValue, formatFxRate)
│       ├── pages/ (8 pages)      # Transactions page: USD Value column + mixed-currency summary
│       └── components/ (6 components)
├── db/migrations/
│   ├── 20260226000001_portal_schema.sql       # Portal schema + 7 tables
│   ├── 20260226000002_portal_tenant_sync.sql  # Trigger: org → tenant sync
│   └── 20260301000001_fx_rate_columns.sql     # FX rate columns on transactions
└── docker-compose.yml            # 6 services: postgres, redis, migrations, bankie, gateway, portal-spa
```

### 3.3 Service Relationships

| Service | Port | Role | Dependencies |
|---------|------|------|-------------|
| `bankie` (Core) | `:3030` | CQRS/ES banking API | PostgreSQL, Redis |
| `bankie-gateway` | `:4040` | API Gateway + Portal API | PostgreSQL, Redis, Core |
| `portal-spa` | `:8080` | Static SPA (nginx) | Gateway |
| PostgreSQL | `:5432` | Event store, views, portal schema | — |
| Redis | `:6379` | Cache, rate limits, locks | — |

### 3.4 Integration Contracts

The Gateway connects to Core through existing interfaces. Core was enhanced with the FX Rate Engine:

| Integration Point | Mechanism | Core Change |
|---|---|---|
| Authentication | Gateway mints JWTs using same `JWT_SECRET` env var | None |
| API proxying | HTTP reverse proxy to `http://bankie-core:3030/v1/*` | None |
| Tenant creation | DB trigger syncs `portal.organizations` → `public.tenants` | None |
| Database | Shared PostgreSQL, Gateway uses `portal` schema | None |
| Redis | Shared Redis, Gateway uses `gw:` prefix, Core uses `fx:` prefix | `fx:USD:*` keys added |
| FX Rate Engine | Core fetches live rates, caches in Redis, writes `amount_usd` on transactions | 3 new cols on `transactions` |
| Settlement CSV | Report includes `amount_usd`, `fx_rate_to_usd`, `fx_rate_source` columns | CSV format extended |

---

## 4. Domain Design

### 4.1 Use Cases

| Actor | Use Case | Flow |
|-------|----------|------|
| **New Tenant** | Self-service signup | POST `/portal/v1/auth/signup` → creates org + owner member + tenant (via DB trigger) |
| **Portal Member** | Login | POST `/portal/v1/auth/login` → argon2id verify → session JWT cookie + CSRF token |
| **Admin** | Create API key | POST `/portal/v1/orgs/:org_id/keys` → generates `bk_live_*` key, returns raw once |
| **Admin** | Rotate key | POST `/portal/v1/orgs/:org_id/keys/:id/rotate` → old key → `rotated` status with 24h grace |
| **Admin** | Revoke key | DELETE `/portal/v1/orgs/:org_id/keys/:id` → immediate revocation |
| **Developer** | API call via key | `Authorization: Bearer bk_live_xxx` → Gateway resolves → rate limit → mint JWT → proxy to Core |
| **Portal User** | View accounts | SPA → `/portal/v1/data/accounts` → data proxy mints JWT → forward GET to Core |

### 4.2 API Key Lifecycle State Machine

```mermaid
stateDiagram-v2
    [*] --> Active: create_key()
    Active --> Rotated: rotate_key()<br/>grace_expires_at = now + 24h
    Active --> Revoked: revoke_key()
    Rotated --> Revoked: grace period expires<br/>OR revoke_key()
    Revoked --> [*]

    note right of Active
        Both Active and Rotated keys
        are valid for API calls
    end note
```

### 4.3 Organization Member Roles

| Role | Org Management | Key Management | View Data | View Logs |
|------|---------------|----------------|-----------|-----------|
| **Owner** | Full CRUD | Full CRUD | Yes | Yes |
| **Admin** | Update | Full CRUD | Yes | Yes |
| **Developer** | Read | Read, Use | Yes | Yes |
| **Member** | Read | Read | Yes | No |
| **Viewer** | Read | — | Yes | No |

### 4.4 API Key Format

```
Format:  bk_live_<32 random base62 chars>   (total: ~41 chars)
Example: bk_live_a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6

Storage: SHA-256(full_key) → portal.api_keys.key_hash
Prefix:  first 16 chars → portal.api_keys.key_prefix (for display)
```

### 4.5 Scope Matrix

| Scope | Grants Access To |
|-------|-----------------|
| `accounts:read` | GET `/v1/bank_account/*`, GET `/v1/accounts`, GET `/v1/user/*` |
| `accounts:write` | POST/PATCH/PUT/DELETE `/v1/bank_account/*` |
| `ledgers:read` | GET `/v1/ledger/*` |
| `transactions:read` | GET `/v1/transaction/*`, GET `/v1/report/*` |
| `house_accounts:read` | GET `/v1/house_account` |
| `house_accounts:write` | POST `/v1/house_account` |

### 4.6 Configuration

| Variable | Source | Default | Description |
|----------|--------|---------|-------------|
| `ENV` | env var | `local` | Config file selector: `config.{ENV}.yaml` |
| `DB_PASSWD` | env var | — | PostgreSQL password |
| `JWT_SECRET` | env var | — | Shared with Core for JWT signing (HS256) |
| `CORE_URL` | env var | `http://localhost:3030` | Upstream Core URL for proxy |
| `GATEWAY_LISTEN_ADDR` | env var | `0.0.0.0:4040` | Gateway bind address |

### 4.7 FX Rate Engine

The FX Rate Engine normalizes all transaction amounts to USD for cross-currency accounting and aggregation.

#### Architecture

```mermaid
graph TB
    subgraph "External Rate Sources"
        CG[CoinGecko API<br/>GET /api/v3/simple/price<br/>?ids=bitcoin,ethereum,tether<br/>&vs_currencies=usd]
        ER[ExchangeRate API<br/>GET /v6/latest/USD<br/>rate inverted: 1/rate]
    end

    subgraph "FxRateService"
        SVC[convert_to_usd<br/>amount, currency → UsdConversion]
        STATIC[Static: USD=1.0]
        CACHE[Redis Cache<br/>fx:USD:BTC TTL 60s<br/>fx:USD:TWD TTL 3600s]
        CGP[CoinGeckoProvider<br/>BTC, ETH, USDT<br/>timeout: 10s]
        ERP[ExchangeRateProvider<br/>TWD<br/>timeout: 3s]
        MOCK[MockProvider<br/>configurable rates]
    end

    SVC -->|1. check| STATIC
    SVC -->|2. check| CACHE
    SVC -->|3. fetch| CGP & ERP
    CGP --> CG
    ERP --> ER
    CGP & ERP -->|4. store| CACHE
```

#### Core Types (`crates/bankie-core/src/common/fx_rate.rs`)

```rust
pub struct FxRate {
    pub from_currency: String,       // e.g., "BTC"
    pub to_currency: String,         // always "USD"
    pub rate: Decimal,               // 1 {from} = rate {to}
    pub source: FxRateSource,
    pub timestamp: DateTime<Utc>,
}

pub enum FxRateSource { Static, CoinGecko, ExchangeRateApi, Backfill, Mock }

pub struct UsdConversion {
    pub fx_rate_to_usd: Decimal,     // rate used
    pub amount_usd: Decimal,         // amount * rate, rounded to 2dp
    pub source: FxRateSource,
}

#[async_trait]
pub trait FxRateProvider: Send + Sync {
    async fn get_usd_rate(&self, currency: &str) -> Result<FxRate, anyhow::Error>;
    fn supported_currencies(&self) -> &[&str];
}
```

#### Conversion Logic

1. **Static**: `currency == "USD"` → `rate = 1.0`, `source = Static`
2. **Cache hit**: Redis GET `fx:USD:{currency}` → deserialize `FxRate`
3. **Provider fetch**: Iterate providers, use first match → cache result
4. **Graceful degradation**: All providers fail → return `None`, transaction proceeds with `amount_usd = NULL`

#### Provider Details

| Provider | Currencies | API URL | Timeout | Cache TTL |
|----------|-----------|---------|---------|-----------|
| CoinGeckoProvider | BTC, ETH, USDT | `api.coingecko.com/api/v3/simple/price` | 10s | 60s |
| ExchangeRateProvider | TWD | `open.er-api.com/v6/latest/USD` | 3s | 3600s |
| MockProvider | All | N/A | N/A | N/A |

#### CoinGecko Coin ID Mapping

`BTC → bitcoin`, `ETH → ethereum`, `USDT → tether`

#### Integration Point

Injected into `BankAccountServices` via builder pattern. Called in `helper::create_transaction_with_journal()` before DB insert:

```rust
let fx_conversion = if let Some(fx_service) = &services.fx_rate_service {
    fx_service.convert_to_usd(amount.amount, &amount.currency.to_string()).await
} else { None };
```

#### Redis Key Pattern

```
Key:     fx:USD:{currency}     (e.g., fx:USD:BTC, fx:USD:TWD)
Value:   JSON-serialized FxRate
TTL:     60s (crypto) | 3600s (fiat)
```

---

## 5. Data Design

### 5.1 Portal Schema (ER Diagram)

All portal tables live in the `portal` PostgreSQL schema. **No foreign keys across schemas** — `tenant_id INTEGER` is the only link to Core's `public.tenants` table.

```mermaid
erDiagram
    portal_organizations ||--o{ portal_org_members : "has members"
    portal_organizations ||--o{ portal_api_keys : "owns keys"
    portal_organizations ||--o{ portal_webhook_endpoints : "configures webhooks"
    portal_api_keys ||--o{ portal_api_logs : "generates logs"
    portal_webhook_endpoints ||--o{ portal_webhook_deliveries : "has deliveries"

    portal_organizations {
        uuid id PK
        int tenant_id UK "auto-assigned via portal.tenant_id_seq"
        varchar name
        varchar slug UK
        varchar status "active|suspended|closed"
        timestamptz created_at
        timestamptz updated_at
    }

    portal_org_members {
        uuid id PK
        uuid org_id FK
        varchar email UK
        varchar password_hash "argon2id"
        varchar role "owner|admin|developer|member|viewer"
        varchar status "active|suspended"
        timestamptz created_at
        timestamptz updated_at
    }

    portal_api_keys {
        uuid id PK
        uuid org_id FK
        int tenant_id
        varchar name
        varchar key_prefix "bk_live_ first 16 chars"
        varchar key_hash UK "SHA-256 of full key"
        jsonb scopes "permission array"
        varchar environment "live|test"
        varchar status "active|rotated|revoked"
        timestamptz grace_expires_at "null if not rotating"
        timestamptz created_at
        timestamptz updated_at
    }

    portal_webhook_endpoints {
        uuid id PK
        uuid org_id FK
        varchar url "HTTPS required"
        varchar signing_secret "HMAC-SHA256 secret"
        jsonb event_types "subscribed events"
        varchar status "active|disabled"
        int failure_count
        timestamptz created_at
        timestamptz updated_at
    }

    portal_webhook_deliveries {
        uuid id PK
        uuid endpoint_id FK
        varchar event_type
        jsonb payload
        int http_status
        int attempt_number
        varchar status "pending|success|failed|dead_letter"
        text response_body
        int latency_ms
        timestamptz next_retry_at
        timestamptz created_at
    }

    portal_api_logs {
        bigint id PK "snowflake"
        uuid api_key_id
        int tenant_id
        varchar method
        varchar path
        int status_code
        int latency_ms
        varchar client_ip
        jsonb request_summary "redacted"
        jsonb response_summary "redacted"
        timestamptz created_at
    }

    portal_audit_logs {
        bigint id PK "snowflake"
        uuid org_id
        uuid actor_id
        varchar action
        varchar resource_type
        varchar resource_id
        jsonb changes
        varchar client_ip
        timestamptz created_at
    }
```

### 5.2 FX Rate Columns on Transactions Table

Migration `20260301000001_fx_rate_columns.sql` adds three nullable columns to the `transactions` table:

```sql
ALTER TABLE transactions
    ADD COLUMN fx_rate_to_usd DECIMAL(24,12),   -- 1 {currency} = X USD (12dp for crypto precision)
    ADD COLUMN amount_usd     DECIMAL(19,2),    -- amount * fx_rate, rounded to 2dp
    ADD COLUMN fx_rate_source  VARCHAR(50);      -- "coingecko", "exchangerate-api", "static", "backfill"

CREATE INDEX idx_transactions_amount_usd ON transactions(amount_usd);
```

| Currency | Amount | fx_rate_to_usd | amount_usd | fx_rate_source |
|----------|--------|---------------|------------|----------------|
| USD | 1000.00 | 1.000000000000 | 1000.00 | static |
| TWD | 50000 | 0.031250000000 | 1562.50 | exchangerate-api |
| BTC | 0.50000000 | 66486.000000000000 | 33243.00 | coingecko |
| ETH | 2.000000000000000000 | 3200.500000000000 | 6401.00 | coingecko |
| USDT | 5000.000000 | 1.000100000000 | 5000.50 | coingecko |

All three columns are nullable — if FX rate fetch fails, the transaction proceeds without USD normalization. A future backfill cron can populate missing values.

### 5.3 Key Indexes

```sql
-- API key lookup (hot path)
CREATE UNIQUE INDEX idx_api_keys_key_hash ON portal.api_keys (key_hash);
CREATE INDEX idx_api_keys_org_id ON portal.api_keys (org_id);
CREATE INDEX idx_api_keys_status ON portal.api_keys (status);

-- Organization lookups
CREATE INDEX idx_organizations_tenant_id ON portal.organizations (tenant_id);
CREATE INDEX idx_organizations_status ON portal.organizations (status);

-- Member lookups
CREATE INDEX idx_org_members_org_id ON portal.org_members (org_id);
CREATE INDEX idx_org_members_email ON portal.org_members (email);

-- Webhook polling
CREATE INDEX idx_webhook_deliveries_status ON portal.webhook_deliveries (status);

-- Log queries (partitioned table)
CREATE INDEX idx_api_logs_tenant_id ON portal.api_logs (tenant_id);
CREATE INDEX idx_api_logs_created_at ON portal.api_logs (created_at);
```

### 5.4 Partitioning Strategy

`portal.api_logs` uses **monthly range partitioning** on `created_at`:

```sql
CREATE TABLE portal.api_logs (...) PARTITION BY RANGE (created_at);

-- Pre-created partitions
CREATE TABLE portal.api_logs_2026_02 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');
-- ... additional monthly partitions
```

**Retention**: 90-day TTL. Drop oldest partition monthly.

### 5.5 Tenant Synchronization

A PostgreSQL trigger automatically creates a `public.tenants` row when a `portal.organizations` row is inserted:

```sql
CREATE TRIGGER trg_sync_org_to_tenant
    AFTER INSERT ON portal.organizations
    FOR EACH ROW
    EXECUTE FUNCTION portal.sync_org_to_tenant();
```

The trigger inserts into `public.tenants` with full default scopes and `ON CONFLICT DO UPDATE` for idempotency. The `portal.tenant_id_seq` starts at 100 to avoid collisions with manually created tenants.

### 5.6 Privacy Considerations

| Data | Classification | Handling |
|------|---------------|----------|
| API key raw value | Secret | Shown once at creation, only SHA-256 hash stored |
| Password | PII | Hashed with argon2id (OWASP recommended) |
| Email | PII | Stored in portal.org_members, not logged |
| API request/response bodies | May contain PII | Pre-redacted at write time in api_logs |
| Client IP | PII | Stored for audit/security, 90-day retention |

---

## 6. APIs Design

### 6.1 Portal Management APIs

#### 6.1.1 Authentication (Public — No Session Required)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/portal/v1/auth/signup` | Create org + owner member |
| POST | `/portal/v1/auth/login` | Verify credentials → session JWT cookie |
| POST | `/portal/v1/auth/logout` | Clear session + CSRF cookies |

**Signup Request/Response:**

```json
// POST /portal/v1/auth/signup
// Request:
{
  "org_name": "Acme Corp",
  "email": "admin@acme.com",
  "password": "securepassword123"
}

// Response: 200 OK
// Set-Cookie: portal_session=<jwt>; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400[; Secure]
// Set-Cookie: csrf_token=<token>; Path=/; SameSite=Lax; Max-Age=86400[; Secure]
{
  "user": {
    "id": "uuid",
    "email": "admin@acme.com",
    "role": "owner",
    "created_at": "2026-02-27T10:00:00Z"
  },
  "organization": {
    "id": "uuid",
    "name": "Acme Corp",
    "slug": "acme-corp",
    "environment": "live",
    "created_at": "2026-02-27T10:00:00Z"
  }
}
```

> **Security**: JWT token is NOT returned in the response body — stored only in HttpOnly cookie. `Secure` flag is set conditionally when `ENV != "local"`.

**Login brute-force protection:**
- Max 5 failed attempts per email within 15-minute window
- Redis key: `login_attempts:{email}` with TTL = 900s
- On lockout: returns `429 Too Many Requests` with `Retry-After: {remaining_seconds}` header
- Fail-open: if Redis unavailable, rate limit check is skipped (availability over security for demo)
- Failed attempts recorded on wrong password AND non-existent email (prevents user enumeration timing)

**Validation rules:**
- `org_name`: non-empty
- `email`: contains `@`, unique across all members
- `password`: minimum 8 characters

#### 6.1.2 Organization Management (Session Required)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/portal/v1/orgs` | Session | Create organization |
| GET | `/portal/v1/orgs/:id` | Session | Get org (own org only) |
| PATCH | `/portal/v1/orgs/:id` | Owner/Admin | Update name/status |

#### 6.1.3 API Key Management (Session Required)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/portal/v1/orgs/:org_id/keys` | Session | Create key (returns raw once) |
| GET | `/portal/v1/orgs/:org_id/keys` | Session | List keys (prefix only, no raw) |
| DELETE | `/portal/v1/orgs/:org_id/keys/:id` | Session + CSRF | Revoke key immediately |
| POST | `/portal/v1/orgs/:org_id/keys/:id/rotate` | Session + CSRF | Rotate with 24h grace |

**Flat route aliases** (org_id inferred from session):
- POST/GET `/portal/v1/api-keys`
- DELETE `/portal/v1/api-keys/:id`
- POST `/portal/v1/api-keys/:id/rotate`

**Create Key Response:**

```json
// POST /portal/v1/orgs/:org_id/keys
// Request:
{
  "name": "Production Key",
  "scopes": ["accounts:read", "accounts:write", "ledgers:read"]
}

// Response: 200 OK (raw_key shown ONCE)
{
  "id": "uuid",
  "name": "Production Key",
  "key_prefix": "bk_live_a1B2c3D4",
  "raw_key": "bk_live_a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6",
  "scopes": ["accounts:read", "accounts:write", "ledgers:read"],
  "created_at": "2026-02-27T10:00:00Z"
}
```

**Rotate Key Response:**

```json
// POST /portal/v1/orgs/:org_id/keys/:id/rotate
// Response: 200 OK
{
  "new_key": {
    "id": "uuid",
    "name": "Production Key (rotated)",
    "key_prefix": "bk_live_x7Y8z9...",
    "raw_key": "bk_live_x7Y8z9...",
    "scopes": ["accounts:read", "accounts:write", "ledgers:read"],
    "created_at": "2026-02-27T10:01:00Z"
  },
  "old_key_id": "uuid",
  "grace_expires_at": "2026-02-28T10:01:00Z"
}
```

#### 6.1.4 Dashboard (Session Required)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/portal/v1/dashboard/stats` | Session | Org stats (real data) |
| GET | `/portal/v1/dashboard/activity` | Session | Recent audit log entries |

**Stats Response (real data from api_logs + api_keys):**

```json
// GET /portal/v1/dashboard/stats
{
  "total_api_keys": 5,
  "active_api_keys": 3,
  "scopes_granted": 4,          // Unique scopes across ACTIVE keys only (HashSet)
  "total_requests_today": 1247, // Count from portal.api_logs in last 24h
  "org_name": "Acme Corp",
  "environment": "live"
}
```

**Activity Feed Response (from portal.audit_logs):**

```json
// GET /portal/v1/dashboard/activity
[
  {
    "id": 1234567890,
    "org_id": "uuid",
    "actor_id": "uuid",
    "action": "api_key.created",      // or "api_key.rotated", "api_key.revoked"
    "resource_type": "api_key",
    "resource_id": "uuid",
    "changes": {"name": "Production Key", "scopes": ["accounts:read"]},
    "created_at": "2026-02-28T10:00:00Z"
  }
]
// Returns 10 most recent entries, ordered by created_at DESC
```

#### 6.1.5 Data Proxy (Session Required)

These routes proxy GET requests to Core using a portal-minted JWT:

| Method | Path | Proxies To |
|--------|------|-----------|
| GET | `/portal/v1/data/accounts` | `/v1/accounts` |
| GET | `/portal/v1/data/accounts/:id` | `/v1/bank_account/:id` |
| GET | `/portal/v1/data/accounts/:id/sub-accounts` | `/v1/bank_account/:id/sub-accounts` |
| GET | `/portal/v1/data/accounts/:id/balance-history` | `/v1/bank_account/:id/balance-history` |
| GET | `/portal/v1/data/transactions` | `/v1/transaction` |
| GET | `/portal/v1/data/ledger/:id` | `/v1/ledger/:id` |
| GET | `/portal/v1/data/reports/settlement` | `/v1/report/settlement` |

### 6.2 Gateway API Key Authentication Flow

```mermaid
sequenceDiagram
    participant T as Tenant App
    participant GW as Gateway :4040
    participant RD as Redis
    participant PG as PostgreSQL
    participant C as Core :3030

    T->>GW: GET /v1/bank_account/123<br/>Authorization: Bearer bk_live_xxx

    Note over GW: API Key Resolver Middleware
    GW->>GW: SHA-256(bk_live_xxx) → key_hash
    GW->>RD: GET gw:api_key:{key_hash}
    alt Cache Hit
        RD-->>GW: {api_key_id, org_id, tenant_id, scopes, env}
    else Cache Miss
        GW->>PG: SELECT FROM portal.api_keys<br/>WHERE key_hash = $1<br/>AND status IN ('active','rotated')
        PG-->>GW: ResolvedApiKey
        GW->>RD: SET gw:api_key:{key_hash} EX 300
    end

    Note over GW: Rate Limiter Middleware
    GW->>RD: rate_limit_check(gw:rate:{api_key_id},<br/>burst=100, sustained=1000/min)
    alt Rate Exceeded
        GW-->>T: 429 Too Many Requests<br/>X-RateLimit-Remaining: 0
    end

    Note over GW: JWT Minter Middleware
    GW->>GW: Mint JWT {iss:"bankie-gateway",<br/>tenant_id, scopes, exp:now+60s}

    Note over GW: Proxy
    GW->>C: GET /v1/bank_account/123<br/>Authorization: Bearer {internal-jwt}
    C-->>GW: 200 {account data}

    GW-->>T: 200 {account data}<br/>X-RateLimit-Limit: 100<br/>X-RateLimit-Remaining: 99<br/>X-RateLimit-Reset: {epoch}
```

### 6.3 Portal Session Authentication Flow

```mermaid
sequenceDiagram
    participant B as Browser
    participant SPA as Portal SPA :8080
    participant GW as Gateway /portal/v1

    B->>SPA: Load dashboard
    SPA->>GW: POST /portal/v1/auth/login<br/>{email, password}
    GW->>GW: argon2id verify password
    GW->>GW: Generate session JWT + CSRF token
    GW-->>SPA: 200 {user, org}<br/>Set-Cookie: portal_session={jwt}<br/>Set-Cookie: csrf_token={token}

    Note over SPA: Subsequent GET requests
    SPA->>GW: GET /portal/v1/api-keys<br/>Cookie: portal_session={jwt}
    GW->>GW: Decode JWT from cookie
    GW-->>SPA: 200 [keys...]

    Note over SPA: Mutating requests require CSRF
    SPA->>GW: POST /portal/v1/api-keys<br/>Cookie: portal_session={jwt}<br/>X-CSRF-Token: {token}
    GW->>GW: Validate CSRF matches JWT claim
    GW-->>SPA: 200 {new key with raw_key}
```

### 6.4 Error Response Format

All errors follow `bankie-common::AppError`:

```json
{
  "code": 400,
  "message": "At least one scope is required"
}
```

| HTTP Status | Error Type | Example | Special Headers |
|-------------|-----------|---------|-----------------|
| 400 | BadRequest | Invalid input, empty name | — |
| 401 | Unauthorized | Missing/invalid auth, wrong password | — |
| 403 | Forbidden | Missing scope, wrong org, CSRF mismatch | — |
| 404 | NotFound | Key/org not found | — |
| 409 | Conflict | Duplicate email, already revoked key | — |
| 429 | TooManyRequests(msg, secs) | Rate limit / login brute-force | `Retry-After: {secs}` |
| 500 | InternalServerError | DB error, JWT signing failure | — |
| 502 | BadGateway | Core unreachable during proxy | — |

**429 response body** includes `retry_after` field:

```json
{
  "code": 429,
  "message": "Too many login attempts. Try again later.",
  "retry_after": 845
}
```

---

## 7. Logging Design

### 7.1 Structured Logging

The Gateway uses `tracing` with structured fields:

| Log Level | Context | Fields |
|-----------|---------|--------|
| `info` | Proxy request | `method`, `upstream_url` |
| `info` | Data proxy | `upstream_url`, `tenant_id` |
| `warn` | Auth failure | Missing/invalid auth header |
| `warn` | Rate limit exceeded | `api_key_id` |
| `warn` | Login brute-force lockout | `email` (hashed), `remaining_secs` |
| `warn` | FX rate fetch failed | `currency`, `provider`, error |
| `warn` | Redis cache miss/error | Cache key, error message |
| `warn` | API key cache invalidation failed | `key_hash` prefix |
| `error` | DB error | Query context, error message |
| `error` | Proxy failure | Upstream URL, error message |
| `error` | All FX providers exhausted | `currency` |

### 7.2 API Request Logging Middleware

The `api_logger` middleware (`middleware/api_logger.rs`) records all API proxy requests to `portal.api_logs`:

```rust
struct ApiLogEntry {
    api_key_id: Uuid,
    tenant_id: i32,
    method: String,
    path: String,
    status_code: i32,
    latency_ms: i32,
    client_ip: Option<String>,   // X-Forwarded-For > X-Real-IP > None
}
```

- Inserted asynchronously via `tokio::spawn` post-response (non-blocking)
- Best-effort: logs warning on DB insert failure, does not propagate error
- Client IP extracted from `X-Forwarded-For` (first value) or `X-Real-IP` header
- Inserted into monthly-partitioned `portal.api_logs` table

### 7.3 Audit Logging

Key management operations (create, rotate, revoke) write to `portal.audit_logs`:

| Action | Resource Type | Changes Recorded |
|--------|--------------|-----------------|
| `api_key.created` | `api_key` | `{name, scopes}` |
| `api_key.rotated` | `api_key` | `{old_key_name, new_key_id, grace_expires_at}` |
| `api_key.revoked` | `api_key` | `{name}` |

- Best-effort async insertion (non-blocking)
- Records `org_id`, `actor_id` (member), `resource_id`, `client_ip`, `changes` JSONB
- Activity feed endpoint returns 10 most recent entries

### 7.4 PII Redaction

- API request/response bodies in `portal.api_logs` are minimal (method, path, status only — no body content)
- Account numbers, amounts, and PII are not logged
- Client IPs retained for security audit (90-day TTL via partition drop)

---

## 8. Security Design

### 8.1 Authentication Mechanisms

| Layer | Mechanism | Details |
|-------|-----------|---------|
| **API Key Auth** (tenant → Gateway) | Bearer token `bk_live_*` | SHA-256 hash lookup, Redis cache 5min |
| **Portal Session Auth** (browser → Gateway) | JWT in HttpOnly cookie | 24h expiry, SameSite=Lax, Secure (non-local) |
| **CSRF Protection** | Token in X-CSRF-Token header | Validated against JWT `csrf` claim on POST/PATCH/PUT/DELETE |
| **Internal JWT** (Gateway → Core) | Short-lived JWT | 60s expiry, `iss: "bankie-gateway"` |
| **Login Brute-Force** | Redis counter per email | 5 attempts / 15-min lockout → 429 + Retry-After |

### 8.2 Password Security

- **Algorithm**: argon2id (OWASP current recommendation, GPU-resistant)
- **Salt**: Cryptographically random per-password via `OsRng`
- **Minimum length**: 8 characters
- **JWT not in response body**: Token stored only in HttpOnly cookie, never exposed to JavaScript

### 8.3 Cookie Security Flags

| Cookie | HttpOnly | SameSite | Secure | Max-Age |
|--------|----------|----------|--------|---------|
| `portal_session` | Yes | Lax | Conditional (`ENV != "local"`) | 86400s (24h) |
| `csrf_token` | No (JS-readable) | Lax | Conditional (`ENV != "local"`) | 86400s (24h) |

The `Secure` flag is set via `is_secure_env()` function — enabled in all environments except `local` development. This ensures cookies are only transmitted over HTTPS in staging/production.

### 8.4 API Key Security

| Concern | Mitigation |
|---------|-----------|
| Key leakage | Raw key shown once at creation; only SHA-256 hash stored |
| Key compromise | Rotation with 24h grace period; immediate revoke available |
| Scope escalation | Scopes stored per-key; scope enforcer validates per-route |
| Brute force | Rate limiting on all API calls; fail-open on Redis error |
| Key enumeration | Prefix is non-secret metadata; hash is the authenticator |
| Stale cache after revoke | Redis DEL on `gw:api_key:{hash}` when key is revoked or rotated |

### 8.5 API Key Cache Invalidation

When a key is revoked or rotated, the Gateway immediately invalidates the Redis cache:

```rust
async fn invalidate_api_key_cache(state: &PortalState, key_hash: &str) {
    let cache_key = format!("gw:api_key:{}", key_hash);
    crate::redis_ops::del_key(client, &cache_key).await;  // Best-effort
}
```

- Called on `revoke_key()` and `rotate_key()` for the old key
- Best-effort: logs warning on failure, doesn't block the operation
- Without this, a revoked key could remain valid in cache for up to 5 minutes

### 8.6 CSRF Prevention

- CSRF token embedded in session JWT `csrf` claim (32-char random alphanumeric)
- Set as non-HttpOnly cookie `csrf_token` (readable by JS)
- Required in `X-CSRF-Token` header for POST/PATCH/PUT/DELETE requests
- Compared against JWT claim server-side; 403 Forbidden on mismatch
- GET requests skip CSRF validation

### 8.7 Login Brute-Force Protection

```mermaid
sequenceDiagram
    participant B as Browser
    participant GW as Gateway
    participant RD as Redis

    B->>GW: POST /portal/v1/auth/login {email, password}
    GW->>RD: GET login_attempts:{email}
    alt Attempts >= 5
        RD-->>GW: count=5
        GW->>RD: TTL login_attempts:{email}
        RD-->>GW: 845 seconds remaining
        GW-->>B: 429 Too Many Requests<br/>Retry-After: 845
    else Attempts < 5
        RD-->>GW: count=2
        GW->>GW: Verify password (argon2id)
        alt Wrong password
            GW->>RD: INCR login_attempts:{email} EX 900
            GW-->>B: 401 Unauthorized
        else Correct
            GW-->>B: 200 OK + Set-Cookie
        end
    end
```

- Redis key: `login_attempts:{email}` with TTL = 900s (15 minutes)
- Failed attempts tracked on wrong password AND non-existent email (prevents timing-based user enumeration)
- Fail-open: if Redis is unavailable, rate limit check is skipped with warning log

### 8.8 Redis Key Namespacing (No Collisions)

| Component | Key Pattern | TTL |
|-----------|------------|-----|
| Core | `outbox_lock` | 600s |
| Core | `balance_snapshot_lock` | 600s |
| Core | `idempotency:{tenant_id}:{key}` | 86400s |
| **Core (FX)** | `fx:USD:{currency}` | 60s (crypto) / 3600s (fiat) |
| **Gateway** | `gw:api_key:{hash}` | 300s |
| **Gateway** | `gw:rate:{api_key_id}` | per-window |
| **Gateway** | `login_attempts:{email}` | 900s |

### 8.9 Internal JWT Security

- Same `JWT_SECRET` as Core (HS256 symmetric)
- 60s TTL prevents replay
- `iss: "bankie-gateway"` distinguishes from direct tenant JWTs
- `aud: "service"` matches Core's validation
- Claims struct matches Core's `Claims` format

---

## 9. Compatibility Design

### 9.1 Backward Compatibility

| Aspect | Status |
|--------|--------|
| Existing Core API contracts | **Additive only** — `amount_usd`, `fx_rate_to_usd`, `fx_rate_source` added to transaction responses (nullable) |
| Existing JWT authentication | **Still works** — Core accepts both direct JWTs and Gateway-minted JWTs |
| Existing tenant data | **Preserved** — portal uses separate `portal` schema |
| Existing transactions | **Preserved** — new FX columns are nullable; old transactions have `NULL` |
| Existing Redis keys | **No collision** — Gateway uses `gw:` prefix, Core FX uses `fx:` prefix |
| Settlement CSV | **Extended** — 3 new columns appended (backward-compatible for parsers using column headers) |

### 9.2 Migration Path

| Phase | Auth Model | Who |
|-------|-----------|-----|
| Current | Direct JWT → Core `:3030` | Existing tenants |
| Post-Portal | API Key → Gateway `:4040` → Internal JWT → Core | New tenants |
| Transition | Both paths active simultaneously | All tenants |
| Future | API Key only (JWT deprecated) | All tenants |

### 9.3 Supported Platforms

| Component | Requirements |
|-----------|-------------|
| Portal SPA | Modern browsers (Chrome 90+, Firefox 88+, Safari 14+, Edge 90+) |
| Gateway API | Any HTTP client supporting Bearer auth |
| Docker | Docker Engine 24+, Docker Compose v2 |

---

## 10. Operations

### 10.1 Docker Compose Stack

```yaml
# 6 services in docker-compose.yml
services:
  postgresql:    # Port 5432 — shared between Core and Gateway
  redis:         # Port 6379 — shared between Core and Gateway
  migrations:    # Runs DB migrations (including portal schema)
  bankie:        # Port 3030 — Core binary
  bankie-gateway: # Port 4040 — Gateway binary
  portal-spa:    # Port 8080 — nginx serving React SPA
```

### 10.2 Capacity Planning

| Metric | Target | Mechanism |
|--------|--------|-----------|
| Gateway throughput | 10,000 req/s per instance | Axum async, Tokio runtime |
| API key resolution (cache hit) | < 1ms p99 | Redis GET |
| API key resolution (cache miss) | < 10ms p99 | PostgreSQL indexed lookup |
| Rate limit check | < 1ms p99 | Redis Lua script (atomic) |
| Gateway PgPool | max_connections=10 | Sized for portal queries only |
| Rate limit defaults | 100 req/s burst, 1000/min sustained | Per API key |

### 10.3 Monitoring

| Metric | Source | Alert Threshold |
|--------|--------|----------------|
| Gateway request latency | tracing spans | p99 > 50ms |
| Rate limit violations | `gw:rate:*` Redis keys | > 100/min per key |
| API key cache hit rate | Redis GET success rate | < 80% |
| Proxy error rate (502s) | Gateway logs | > 1% |
| Login failure rate | Auth handler logs | > 10/min per IP |
| Login lockouts (429s) | Auth handler logs | > 5/hour |
| FX rate fetch failures | FxRateService logs | > 3 consecutive failures |
| Transactions without USD amount | `amount_usd IS NULL` count | > 5% of recent transactions |
| DB connection pool usage | PgPool metrics | > 80% capacity |

### 10.4 Gateway Middleware Stack

**Portal routes** (`/portal/v1/*`):
```
Public: auth_routes (login/signup/logout) — no middleware (brute-force protection within handler)
Protected: session_auth (cookie JWT + CSRF validation) → route handlers
```

**API proxy routes** (`/*` fallback):
```
api_key_resolver → rate_limiter → api_logger → jwt_minter → reverse_proxy → Core :3030
```

### 10.5 Rollout Plan

1. **Phase 1 (Complete)**: Gateway core + Portal API + SPA — API key lifecycle, org management, data proxy
2. **Phase 2 (Complete)**: Security hardening — brute-force protection, cookie security, cache invalidation, API/audit logging
3. **Phase 3 (Complete)**: FX Rate Engine — USD normalization, cross-currency aggregation, settlement CSV with FX columns
4. **Phase 4**: RBAC enforcement, sandbox environment (`bk_test_*` keys), member management
5. **Phase 5**: Webhook delivery (schema already defined), production hardening

### 10.6 Fallback Plan

| Scenario | Fallback |
|----------|----------|
| Gateway down | Tenants fall back to direct JWT auth against Core `:3030` |
| Redis down | Rate limiter fails open; API key resolution falls through to DB; FX cache bypassed (direct provider fetch) |
| FX rate providers down | Transaction proceeds with `amount_usd = NULL`; backfill cron can fill later |
| Portal SPA down | API key auth still works; management via direct API calls |
| Core unreachable | Gateway returns 502; SPA data proxy shows error state |
| Login lockout (Redis down) | Brute-force check skipped (fail-open for availability) |

### 10.7 Testing

| Test Type | Count | Scope |
|-----------|-------|-------|
| Unit tests (Core) | 126+ | Aggregates, commands, views, report, FX rate service + providers |
| Unit tests (Gateway) | 104+ | Middleware (incl. api_logger), routes, auth, models, repo |
| Unit tests (Common) | 8 | AppError (incl. TooManyRequests) |
| E2E tests (Core) | 59 | Full banking lifecycle |
| E2E tests (Portal) | Planned | Signup → create key → API call → data proxy |

**Testing patterns:**
- Repository traits with `mockall::automock` for unit testing
- `tower::ServiceExt::oneshot` for route handler testing
- Session JWT fixtures for auth middleware tests
- Scope enforcement tested with mock repositories
- FX rate tests use `MockProvider` with configurable static rates
- FX conversion tests cover rounding (2dp USD), tiny fiat (TWD), stablecoins (USDT ~1.0), serialization round-trip

### 10.8 Infrastructure Requirements

| Resource | Current | After Portal |
|----------|---------|-------------|
| PostgreSQL | 1 instance | Same instance, `portal` schema added |
| Redis | 1 instance | Same instance, `gw:*` keys added |
| Docker services | 4 (postgres, redis, migrations, bankie) | 6 (+gateway, +portal-spa) |
| Ports exposed | `:3030`, `:5432`, `:6379` | +`:4040`, +`:8080` |

---

## Appendix A: Valid Scopes

```rust
const VALID_SCOPES: &[&str] = &[
    "accounts:read",
    "accounts:write",
    "ledgers:read",
    "ledgers:write",
    "transactions:read",
    "transactions:write",
    "house_accounts:read",
    "house_accounts:write",
    "reports:read",
];
```

## Appendix B: Competitive Reference

| Feature | Stripe | Column | **Bankie** |
|---------|--------|--------|-----------|
| Key prefix | `sk_live_` / `sk_test_` | `col_` | `bk_live_` / `bk_test_` |
| Key hashing | SHA-256 | SHA-256 | SHA-256 |
| Key rotation | Grace period | Grace period | 24h grace period |
| Rate limiting | Per-key | Per-key | Per-key (100 burst, 1000/min) |
| Portal auth | Password + 2FA | SSO | Password (argon2id) |
| RBAC | Owner/Admin/Dev/Viewer | Admin/Dev | Owner/Admin/Dev/Member/Viewer |
| Webhook signing | HMAC-SHA256 | HMAC-SHA256 | HMAC-SHA256 (planned) |

## Appendix C: Implemented vs. Future Work

### Implemented

| Item | PR | Description |
|------|-----|-------------|
| API request logging | PR #13 | `api_logger` middleware → async insert into partitioned `portal.api_logs` |
| Audit logging | PR #13 | Append-only tracking of key create/rotate/revoke in `portal.audit_logs` |
| Dashboard real data | PR #13 | 24h API call count, scopes-granted count, activity feed |
| Login brute-force protection | PR #15 | 5 attempts / 15-min lockout → 429 + Retry-After |
| Cookie security hardening | PR #15 | Conditional `Secure` flag, JWT removed from response body |
| API key cache invalidation | PR #15 | Redis DEL on revoke/rotate |
| FX Rate Engine | PR #16 | CoinGecko + ExchangeRate providers, Redis cache, `amount_usd` on transactions |
| Settlement CSV FX columns | PR #16 | `amount_usd`, `fx_rate_to_usd`, `fx_rate_source` in CSV |
| Portal USD display | PR #16 | USD Value column on Transactions, mixed-currency balance summary |

### Future Work

| Item | Phase | Description |
|------|-------|-------------|
| Webhook dispatcher | Phase 5 | Outbox poll → HMAC delivery → retry with backoff |
| Sandbox environment | Phase 4 | `bk_test_*` keys → isolated test tenant |
| Member CRUD | Phase 4 | Invite, role management, status management |
| FX rate backfill cron | Future | Populate `amount_usd` for historical transactions missing FX data |
| Historical FX rates | Future | Use transaction-time rates instead of live rates for backfill |
| OAuth2/OIDC | Future | Google/GitHub SSO for portal login |
| IP allowlisting | Future | Restrict API key usage to specific CIDRs |
| Usage analytics | Future | API call trends, error rate charts in dashboard |

## Appendix D: Settlement CSV Format (15 Columns)

```
transaction_date,value_date,transaction_reference,transaction_type,debit_amount,credit_amount,
currency,amount_usd,fx_rate_to_usd,fx_rate_source,description,status,running_balance,
account_number,journal_entry_id
```

| Column | Type | Description |
|--------|------|-------------|
| `transaction_date` | DateTime | Transaction timestamp |
| `value_date` | Date | Journal entry date |
| `transaction_reference` | String | `DE-xxx`, `WI-xxx`, `TR-xxx` |
| `transaction_type` | String | `deposit`, `withdrawal`, `transfer` |
| `debit_amount` | Decimal | Debit (source of funds) |
| `credit_amount` | Decimal | Credit (use of funds) |
| `currency` | String | Native currency (USD, BTC, etc.) |
| **`amount_usd`** | Decimal (2dp) | USD-equivalent amount (nullable) |
| **`fx_rate_to_usd`** | Decimal (12dp) | Exchange rate used (nullable) |
| **`fx_rate_source`** | String | Rate source: coingecko, exchangerate-api, static |
| `description` | String | Transaction description |
| `status` | String | posted, processing, failed |
| `running_balance` | Decimal | Cumulative balance |
| `account_number` | String | Bank account number |
| `journal_entry_id` | UUID | Double-entry journal reference |

Report includes UTF-8 BOM for Excel compatibility, metadata header with account/period/opening balance, and summary footer with totals.

## Appendix E: Portal SPA Currency Utilities

Shared formatting functions in `portal-spa/src/utils/currency.ts`:

| Function | Input | Output | Example |
|----------|-------|--------|---------|
| `formatUsdValue(value)` | `"32716.05"` or null | `"$32,716.05"` or null | USD amounts with 2dp |
| `formatFxRate(rate)` | `"65432.10"` or null | `"@ 65,432.10"` or null | Suppressed if rate = 1.0 |

**Transactions page USD display:**
- **USD Value column**: Shows `formatUsdValue(tx.amount_usd)` with `formatFxRate(tx.fx_rate_to_usd)` below
- **Balance summary cards**: When accounts have mixed currencies, labels change to "(USD Equivalent)" and aggregate using native amounts as-is (until backend provides `available_usd` per account)
