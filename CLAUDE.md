# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Bankie is a multi-tenant banking/ledger system built in Rust as a **Cargo workspace** with three crates. It implements sub-account and ledger management using **CQRS/Event Sourcing** (via `cqrs-es` + `postgres-es`), a **Developer Portal Gateway** with session auth and API key management, and a **Portal SPA** (React + TypeScript). Supports both fiat (USD, TWD) and crypto assets (BTC, ETH, USDT) with currency-aware precision.

## Workspace Structure

```
crates/
├── bankie-core/     — Banking engine: CQRS aggregates, REST API (:3030), JWT tenant auth
├── bankie-common/   — Shared types (AppError)
└── bankie-gateway/  — Developer Portal: session auth, API key CRUD, rate limiting, reverse proxy (:4040)
portal-spa/          — React SPA: dashboard, API keys, org settings, data views (:8080 via nginx)
```

## Build & Development Commands

```bash
# Build (use SQLX_OFFLINE=true when no live DB available)
SQLX_OFFLINE=true cargo build
cargo build --release --bin bankie
cargo build --release --bin bankie-gateway

# Run tests (410 unit tests: 131 core + 270 gateway + 9 common, no DB needed)
SQLX_OFFLINE=true cargo test -- --nocapture
cargo test -p bankie-core test_name -- --nocapture    # Single test in specific crate
cargo test -p bankie-gateway test_name -- --nocapture
cargo llvm-cov nextest                                # Coverage (requires cargo-nextest + cargo-llvm-cov)

# Lint (must pass CI — clippy treats warnings as errors)
cargo clippy --all-targets --tests --benches --no-default-features -- -D warnings
cargo fmt -- --check
cargo check --all

# Regenerate sqlx offline cache (REQUIRED after changing any SQL query)
# Core queries cached in crates/bankie-core/.sqlx/
DATABASE_URL="postgres://bankie_app:password@localhost:5432/bankie_main" cargo sqlx prepare

# E2E tests (59 tests, requires running stack)
make docker-up && make docker-e2e        # Docker full-stack
make local-setup && make local-e2e       # Local dev

# Docker lifecycle (6 services: postgres, redis, migrations, bankie-core, bankie-gateway, portal-spa)
make docker-up                           # Build and start full stack
make docker-down                         # Stop (preserves data)
make docker-clean                        # Stop + remove volumes (full data reset)

# Local dev lifecycle
make local-setup                         # One-shot: infra + db + build + jwt + server
make local-gateway                       # Start gateway server (background)
make local-portal                        # Start SPA dev server (:5173)
make local-stop                          # Stop server + gateway + tear down infra

# Portal SPA development
cd portal-spa && npm install && npm run dev   # Vite dev server on :5173 (proxies /api to :4040)
cd portal-spa && npm run build                # Production build to dist/

# Demo (via Gateway, auto-creates portal org + API key)
make docker-up && ./scripts/demo.sh     # Docker full-stack
make local-setup && make local-gateway && make local-demo  # Local dev

# Core API tests (direct to :3030 with JWT, no Gateway)
make docker-core-test                    # Against Docker stack
make local-core-test                     # Against local dev

# Interactive testing console (menu-driven, all API operations via Gateway)
make docker-interactive                  # Against Docker stack
make local-interactive                   # Against local dev

# Generate JWT (for core-test.sh and e2e-test.sh)
cargo run --bin bankie -- --mode secret_key              # Generate secret
cargo run --bin bankie -- --mode jwt --service {name}    # Generate tenant JWT

# Concurrency pressure testing (requires k6)
make over-withdrawn-test
```

## Infrastructure Dependencies

- **PostgreSQL 16** — event store, views, transactions, journals, outbox, balance snapshots, portal schema
- **Redis** — distributed lock, idempotency dedup, API key cache, rate limiting (token bucket via Lua script)
- **Config files**: `config.{ENV}.yaml` for core, `config.gateway.{ENV}.yaml` for gateway (ENV defaults to `local`, set to `docker` in containers)
- **Env vars**: `DB_PASSWD`, `JWT_SECRET`, `CORE_URL`, `RUST_LOG`, `SQLX_OFFLINE=true`
- **Docker Compose**: 6 services — PostgreSQL (:5432), Redis (:6379), migrations, bankie-core (:3030), bankie-gateway (:4040), portal-spa/nginx (:8080)

## Architecture

### System Overview

```
Portal SPA (React :8080)
  └→ nginx proxy /api/portal/* → Gateway :4040/portal/*

Gateway (:4040)
  ├→ Portal API (/portal/v1/*) — session cookie auth + CSRF
  │   ├→ Auth: signup, login, logout
  │   ├→ Org CRUD, member management
  │   ├→ API key lifecycle: create, rotate (grace period), revoke
  │   ├→ Dashboard stats
  │   └→ Data proxy: mints 60s JWT → forwards to Core
  └→ External API (/*) — API key auth
      └→ api_key_resolver → rate_limiter → jwt_minter → reverse_proxy → Core :3030

Core (:3030) — CQRS/Event Sourcing banking engine
  └→ JWT tenant auth → all operations scoped to tenant_id
```

### Dual Auth Model

**Portal SPA users** (session-based):
- `POST /portal/v1/auth/login` → argon2id password verification → `portal_session` HttpOnly cookie + `csrf_token` cookie
- CSRF double-submit: `X-CSRF-Token` header validated against cookie for mutating requests
- `SessionClaims`: sub (member_id), org_id, tenant_id, role, csrf, exp (24h)

**External API consumers** (API key-based):
- `Authorization: Bearer bk_live_...` → SHA-256 hash lookup → `ResolvedApiKey` (org_id, tenant_id, scopes)
- Gateway mints short-lived internal JWT (60s, `iss: "bankie-gateway"`) matching Core's Claims format
- Rate limiting: Redis token bucket (burst 100, sustained 1000/min) per API key, throttle count tracked in `gw:throttled:{api_key_id}` (24h window) for dashboard metrics

### CQRS/Event Sourcing Core

Two aggregates using `cqrs-es`/`postgres-es`:

**BankAccount aggregate** (`crates/bankie-core/src/event_sourcing/aggregate/bank_account.rs`)
- Commands: `OpenAccount`, `ApproveAccount`, `FreezeAccount`, `UnfreezeAccount`, `CloseAccount`, `Deposit`, `Withdrawal`, `Transfer`
- Commands sent via a **bounded** `mpsc` channel (capacity 10,000) and processed sequentially
- Deposit/Withdrawal create transactions + journal entries + outbox records (not aggregate events)
- Account lifecycle: `Pending` → `Approved` → `Freeze`/`CustomerClosed`

**Ledger aggregate** (`crates/bankie-core/src/event_sourcing/aggregate/ledger.rs`)
- Commands: `Init`, `Credit`, `DebitHold`, `DebitRelease`
- Tracks `available`, `pending`, and `current` balances using delta-based updates
- Withdrawal uses debit-hold pattern: moves funds from `available` to `pending`, releases via outbox job

### Tenant Isolation

Every entity carries `tenant_id`. Two paths:

**Core path** (direct JWT):
```
JWT claims → auth middleware → Extension<i32> → CommandExtractor.set_tenant_id()
  → Command.tenant_id → Aggregate → BaseEvent.tenant_id → View.update()
  → JSON payload → DB trigger → indexed tenant_id column
```

**Gateway path** (API key → minted JWT):
```
API key → SHA-256 hash → DB lookup → ResolvedApiKey.tenant_id
  → jwt_minter (60s internal JWT) → Core auth middleware → same flow as above
```

**Portal path** (session → data proxy):
```
Session cookie → SessionClaims.tenant_id → data_proxy mints JWT → Core
```

Key design decisions:
- `#[serde(skip_deserializing)]` on `tenant_id` in commands — only server-side injection
- DB triggers sync `tenant_id` from cqrs-es JSON `payload` to indexed columns
- Portal organizations auto-sync to Core tenants via PostgreSQL trigger (`portal.organizations` INSERT → `public.tenants`)

### Portal Schema (in `portal` PostgreSQL schema)

8 tables: `organizations`, `org_members` (with `name`, `invite_token_hash`, `invite_expires_at`), `api_keys`, `webhook_endpoints`, `webhook_deliveries`, `webhook_events` (staging table with DB triggers for fan-out), `api_logs` (range-partitioned by month), `audit_logs`. Migrations at `db/migrations/20260226*.sql` (base schema + tenant sync), `20260302*.sql` (invite tokens + member name), and `20260303*.sql` (Phase 3: webhook_events staging, outbox triggers, api_logs partitions).

### Gateway Middleware Stack

**Portal routes** (`/portal/v1/*`):
```
Public: auth_routes (login/signup/logout) — no middleware
Protected: session_auth (cookie JWT + CSRF validation)
  → rbac middleware (role-based: Owner/Admin/Member)
  → org/api-key/member/dashboard/data-proxy handlers
```

**API proxy routes** (`/*` fallback):
```
api_key_resolver (Bearer → DB lookup + Redis cache 5min, invalidated on revoke/rotate)
  → rate_limiter (Redis token bucket via Lua script)
  → api_logger (async write to portal.api_logs, PII-redacted IPs + sensitive query params)
  → jwt_minter (60s internal JWT)
  → reverse_proxy → Core :3030
```

### Core Middleware Stack

```
TraceLayer → CompressionLayer → AddExtension(State) → AddExtension(Redis)
  → authorize (JWT auth + tenant_id extraction)
  → idempotency_check (Redis SET NX EX, 24h TTL, tenant-scoped)
  → Handler
```

### Multi-Asset Support

- `Currency` enum (USD, TWD, BTC, ETH, USDT) with per-currency precision (2, 0, 8, 18, 6)
- `AssetRegistry` — `Arc<RwLock<HashMap<String, Asset>>>` loaded at startup
- `Money` type carries `amount: Decimal` + `currency: Currency`, use `money.asset_code()`

### FX Rate Engine

Real-time USD normalization for multi-currency transactions (`crates/bankie-core/src/common/fx_rate.rs`):

- **FxRateProvider trait** with three implementations:
  - `CoinGeckoProvider` — crypto rates (BTC, ETH, USDT), 60s cache TTL
  - `ExchangeRateProvider` — fiat rates (TWD), 1h cache TTL
  - `MockProvider` — configurable static rates for tests
- **FxRateService** — coordinates providers with Redis caching (`fx:USD:{CURRENCY}`)
- **Integration**: wired into `BankAccountServices` via builder pattern (`with_fx_rate_service()`), called in `helper::create_transaction_with_journal()` before DB insert
- **Transaction fields**: `fx_rate_to_usd Option<Decimal>`, `amount_usd Option<Decimal>`, `fx_rate_source Option<String>` — all nullable for graceful degradation
- **Graceful degradation**: FX rate failure never blocks a transaction — proceeds with NULL USD fields
- **Settlement CSV**: includes `amount_usd`, `fx_rate_to_usd`, `fx_rate_source` columns

### Key Data Flows

**Banking operation** (via Core or Gateway proxy):
```
HTTP Request → Auth → Idempotency Check → Route Handler
  → CommandExtractor (deserialize + generate IDs + validate amounts > 0 + inject tenant_id)
  → mpsc channel (bounded, 10k) → Aggregate::handle()
    → BankAccountServices (validates status/balance, creates transactions/journals/outbox)
    → Outbox cron job → Ledger aggregate (credit/debit release)
```

**API key rotation** (via Portal):
```
POST /portal/v1/api-keys/:id/rotate → creates new key (active) + marks old key (rotated)
  → grace_expires_at set (default 24h) → both keys valid during grace period
  → after grace period, old key stops working
```

### Double-Entry Bookkeeping

Every deposit/withdrawal creates a `Transaction` + `JournalEntry` + `JournalLine`s. Journal lines have debit/credit amounts linking user ledger and house account ledger.

### Sub-Account Architecture

Multiple account types (Checking, Interest, Yield) linked via `parent_id`. Master account (Checking) has empty `parent_id`; sub-accounts point to the master.

**View projection gotcha**: When adding new event handlers in `query.rs`, never unconditionally overwrite `parent_id` (or similar fields set by earlier events). Only update if the event value is non-empty.

### Outbox Pattern + Cron Jobs

| Job | Schedule | Purpose |
|-----|----------|---------|
| Ledger job | Every 10s | Polls outbox → executes `LedgerCommand::Credit` or `DebitRelease`. Redis-locked. |
| Balance snapshot | Daily midnight UTC | Snapshots all active account balances. Redis-locked. |
| API logging | Per request | Async write to `portal.api_logs` (partitioned by month) via `api_logger` middleware. PII redacted (IP masking, sensitive query param stripping). |
| Grace expiry | Every 60s | Revokes rotated API keys past grace period. Redis-locked (`gw:lock:grace_expiry`). Gateway-side (`crates/bankie-gateway/src/job.rs`). |
| Webhook fan-out | Every 5s | Polls `webhook_events` staging table → creates delivery records for matching endpoints. Redis-locked (`gw:lock:webhook_fanout`). |
| Webhook delivery | Every 5s | Sends pending webhook deliveries via HTTP POST with HMAC-SHA256 signing. Exponential backoff retry (30s→24h, 7 attempts max), circuit breaker (5 failures → disable endpoint), dead-letter. Redis-locked (`gw:lock:webhook_deliver`). |

### RBAC (Role-Based Access Control)

Three roles in `MemberRole` enum (`crates/bankie-gateway/src/middleware/rbac.rs`):
- **Owner** — full permissions, cannot be removed or demoted
- **Admin** — manage members (except Owner/other Admins), manage API keys and org settings
- **Member** — read-only access

RBAC middleware functions: `require_member_management()`, `require_api_key_management()`, `require_org_management()` — each checks `SessionClaims.role` and returns 403 if insufficient.

### Member Management & Invite Flow

Members (`portal.org_members`) have statuses: `Active`, `Pending`, `Suspended`.

**Invite flow** (`crates/bankie-gateway/src/routes/member.rs`):
```
POST /members/invite → generate 32-byte token → store SHA-256 hash + 7-day expiry
  → return invite_link with raw token (shown once)
  → GET /auth/invite?token=... → validate hash + expiry → show accept form
  → POST /auth/invite/accept → set password (argon2id) + activate → session cookies
```

Role assignment rules: Owner can assign Admin/Member; Admin can only assign Member. Cannot change Owner's role. Admin cannot remove other Admins.

### Security Hardening

- Session cookies (`portal_session`, `csrf_token`) include `Secure` flag when `ENV != local`
- JWT token removed from login/signup response body — auth is cookie-only
- `tenant_id` removed from `CreateOrgRequest` — auto-assigned via DB sequence
- Login brute-force protection: Redis INCR with 15min TTL, max 5 failed attempts per email, returns 429 + `Retry-After`
- API key cache (`gw:api_key:{hash}`) actively deleted on revoke/rotate — no stale cache window

### Webhook System

Event-driven webhook delivery pipeline (`crates/bankie-gateway/src/webhook/`):

**Architecture**: DB trigger → staging table → fan-out → delivery with retry
```
DB trigger (bank_account_events INSERT) → webhook_events staging table
  → Fan-out job (5s) → matches endpoints by tenant_id + event_type → webhook_deliveries
  → Delivery job (5s) → HTTP POST with HMAC-SHA256 signing → retry/circuit-break/dead-letter
```

**HMAC-SHA256 Signing** (`webhook/signing.rs`): Stripe-compatible format `t={timestamp},v1={hex_signature}`. Message = `{timestamp}.{payload}`. Header: `X-Bankie-Signature`.

**Retry Strategy** (`webhook/deliverer.rs`): Exponential backoff [30s, 2m, 15m, 1h, 4h, 12h, 24h] with ±10% jitter. Max 7 attempts. Circuit breaker disables endpoint after 5 consecutive failures. Dead-letter after max attempts.

**WebhookRepository** (`repo/webhook.rs`): 20-method trait covering endpoint CRUD, secret rotation, circuit breaker state, fan-out queries, staging events, delivery lifecycle, and API log insertion. PgWebhookRepository implements all SQL.

**PII Redaction** (`middleware/api_logger.rs`): IPv4 masked to /24 subnet, IPv6 to /48 subnet. Sensitive query params (token, secret, password, key, api_key, credential) replaced with `[REDACTED]`. Applied at storage time and on read (double protection).

### Structured Error Responses

`AppError` enum in `crates/bankie-common/src/error.rs` (shared by both core and gateway):
- `BadRequest(400)`, `Unauthorized(401)`, `Forbidden(403)`, `NotFound(404)`, `Conflict(409)`, `TooManyRequests(429)`, `UnprocessableEntity(422)`, `InternalServerError(500)`

## API Endpoints

### Core (:3030)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | No | Liveness check |
| GET | `/ready` | No | Readiness check (DB + Redis) |
| GET | `/v1/bank_account/:id` | JWT | Query bank account view |
| GET | `/v1/bank_account/:id/sub-accounts` | JWT | List sub-accounts |
| GET | `/v1/bank_account/by-number/:account_number` | JWT | Lookup by account number |
| POST | `/v1/bank_account` | JWT | Execute bank account command |
| GET | `/v1/ledger/:id` | JWT | Query ledger view |
| GET | `/v1/house_account?currency=` | JWT | List house accounts |
| POST | `/v1/house_account` | JWT | Create house account |
| GET | `/v1/user/:id` | JWT | Query user's accounts with ledger |
| GET | `/v1/accounts?offset=&limit=` | JWT | List accounts (paginated, max 100) |
| GET | `/v1/transaction?bank_account_id=&offset=&limit=` | JWT | List transactions |
| GET | `/v1/bank_account/:id/balance-history` | JWT | Balance history from snapshots |
| GET | `/v1/report/settlement` | JWT | Settlement report CSV (max 90-day range) |

### Gateway (:4040) — Portal API

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/portal/v1/auth/signup` | None | Create org + owner member |
| POST | `/portal/v1/auth/login` | None | Login → session cookie |
| POST | `/portal/v1/auth/logout` | None | Clear cookies |
| GET | `/portal/v1/dashboard/stats` | Session | Org stats, key counts, scopes granted, 24h API calls |
| GET | `/portal/v1/dashboard/activity` | Session | Recent audit log activity feed |
| GET | `/portal/v1/organization` | Session | Get org details |
| PUT | `/portal/v1/organization` | Session | Update org |
| GET | `/portal/v1/api-keys` | Session | List API keys |
| POST | `/portal/v1/api-keys` | Session | Create key (returns raw key once) |
| POST | `/portal/v1/api-keys/:id/rotate` | Session | Rotate key (grace period) |
| DELETE | `/portal/v1/api-keys/:id` | Session | Revoke key |
| GET | `/portal/v1/members` | Session | List org members |
| POST | `/portal/v1/members/invite` | Session+RBAC | Invite member (Owner/Admin) |
| POST | `/portal/v1/members/:id/role` | Session+RBAC | Change member role |
| DELETE | `/portal/v1/members/:id` | Session+RBAC | Remove member |
| POST | `/portal/v1/members/:id/resend-invite` | Session+RBAC | Resend invite to pending member |
| GET | `/portal/v1/auth/invite` | None | Validate invite token |
| POST | `/portal/v1/auth/invite/accept` | None | Accept invite + set password |
| GET | `/portal/v1/data/*` | Session | Data proxy → Core (accounts, transactions, reports) |
| GET | `/portal/v1/logs` | Session | API logs viewer (paginated, filtered by method/status/path) |
| GET | `/portal/v1/webhook-endpoints` | Session | List webhook endpoints |
| POST | `/portal/v1/webhook-endpoints` | Session+RBAC | Create webhook endpoint (returns signing secret once) |
| PUT | `/portal/v1/webhook-endpoints/:id` | Session+RBAC | Update webhook endpoint |
| DELETE | `/portal/v1/webhook-endpoints/:id` | Session+RBAC | Delete webhook endpoint |
| POST | `/portal/v1/webhook-endpoints/:id/rotate-secret` | Session+RBAC | Rotate signing secret |
| GET | `/portal/v1/webhook-endpoints/:id/deliveries` | Session | List deliveries for endpoint (paginated, filtered by status) |

### Gateway (:4040) — External API Proxy

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| ANY | `/*` | API Key | Proxied to Core with minted JWT |

## Portal SPA

React 19 + Vite + TailwindCSS 4 + TanStack Query. Source at `portal-spa/src/`.

- **API client** (`api/client.ts`): base URL `/api/portal/v1`, CSRF token from cookie, credentials `same-origin`
- **Auth** (`hooks/useAuth.ts`): context provider, login/signup/logout, 401 redirect
- **Pages**: Login, Signup, Dashboard, ApiKeys, Organization, Members, AcceptInvite, Accounts, Transactions (with USD Value column + FX rates), Reports, ApiDocs, Webhooks (endpoint CRUD + delivery history), Logs (API log viewer with filters)
- **Utilities** (`utils/currency.ts`): shared currency formatting with per-currency precision, USD value + FX rate formatters
- **Types** (`types/index.ts`): TypeScript interfaces matching gateway response shapes
- **Vite proxy**: `/api` → `http://localhost:4040` (dev only; production uses nginx)
- **Production**: nginx serves SPA at `:80`, proxies `/api/portal/` → `http://bankie-gateway:4040/portal/`

## Testing Patterns

- **410 unit tests** (131 core + 270 gateway + 9 common) + **59 e2e tests**, unit tests need no DB (`SQLX_OFFLINE=true`)
- Core aggregate tests use `cqrs_es::test::TestFramework` with given/when/then pattern and `test_case!`/`test_error_case!` macros
- `MockBankAccountServices` (manual mock) with `Mutex<Option<Result<...>>>` fields for negative-path testing
- Core DB layer uses `mockall::automock` on `DatabaseClient` trait
- Gateway uses `mockall` on `OrgRepository`, `MemberRepository`, `ApiKeyRepository`, `WebhookRepository` traits
- Gateway middleware tests (session, JWT minter, scope enforcer) use mock repos + tower's `oneshot`
- Webhook tests: 20 model unit tests, 17 repo trait tests, 14 signing tests, 16 route tests, 5 dispatcher tests, 8 deliverer tests, 15 PII redaction tests, 9 logs route tests
- E2E tests (`scripts/e2e-test.sh`) cover full banking lifecycle (direct Core API with JWT)
- Core API tests (`scripts/core-test.sh`) — automated direct Core API testing with JWT auth
- Demo script (`scripts/demo.sh`) — full lifecycle via Gateway with API key auth (auto-creates portal org)
- Interactive console (`scripts/interactive.sh`) — menu-driven, routes Core API calls through Gateway with API key auth

## SQLx Offline Mode

Compile-time checked queries via `sqlx`. Core cache at `crates/bankie-core/.sqlx/`. When any SQL query changes, regenerate with a running DB:
```bash
DATABASE_URL="postgres://bankie_app:password@localhost:5432/bankie_main" cargo sqlx prepare
```

## Pre-commit Hooks

`.pre-commit-config.yaml`: `cargo fmt`, `cargo check`, `cargo clippy` (with `-D warnings`), `cargo test`.

## Three Binary Targets

- `bankie` (`crates/bankie-core/src/main.rs`) — Core banking server (:3030). CLI modes: `server`, `secret_key`, `jwt`. Graceful shutdown via `SIGINT`.
- `bankie-gateway` (`crates/bankie-gateway/src/main.rs`) — Developer Portal gateway (:4040). Graceful shutdown via `SIGINT`.
- `migrations` (`crates/bankie-core/src/repository/migrate.rs`) — Database migration runner.
