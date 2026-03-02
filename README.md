# Bankie

Multi-tenant banking system with a Developer Portal, built in Rust. Implements sub-account and ledger management using CQRS/Event Sourcing (`cqrs-es` + `postgres-es`), with an API gateway for portal management and external API access.

## Architecture

```
┌─────────────────┐     ┌──────────────────────┐     ┌─────────────────┐
│   Portal SPA    │────▶│   Gateway (:4040)     │────▶│  Core (:3030)   │
│  React (:8080)  │     │  Session + API Key    │     │  CQRS/ES Engine │
└─────────────────┘     │  Auth, Rate Limiting  │     └─────────────────┘
                        └──────────────────────┘
                                  │
                        ┌─────────┴─────────┐
                        │  PostgreSQL + Redis │
                        └───────────────────┘
```

**Cargo workspace** with three crates:
- **bankie-core** — Banking engine: CQRS aggregates, REST API, JWT tenant auth
- **bankie-gateway** — Developer Portal: session auth, API key CRUD, rate limiting, reverse proxy
- **bankie-common** — Shared types (AppError)
- **portal-spa** — React + Vite + TailwindCSS dashboard

## Features

### Banking Core
- **Multi-tenant** JWT-based authentication with tenant isolation across all queries
- **CQRS/Event Sourcing** with BankAccount and Ledger aggregates
- **Sub-accounts** — Checking (master), Interest, Yield linked via `parent_id`
- **Double-entry bookkeeping** — every transaction creates journal entries with debit/credit lines
- **Multi-asset support** — USD, TWD, BTC, ETH, USDT with currency-aware precision
- **FX Rate Engine** — real-time USD normalization via CoinGecko (crypto) and ExchangeRate API (fiat), Redis-cached, graceful degradation
- **Outbox pattern** — async ledger processing with Redis distributed locking
- **Idempotency** — Redis-backed `Idempotency-Key` header deduplication
- **Settlement reports** — CSV export with running balances, FX rate columns, and CSV-injection prevention
- **Balance snapshots** — daily snapshots for historical balance queries

### Developer Portal
- **Session auth** — argon2id password hashing, HttpOnly Secure session cookies, CSRF double-submit
- **Organization management** — signup, org CRUD, member invite/revoke with RBAC (owner/admin/member)
- **API key lifecycle** — create (raw key shown once), rotate (with grace period), revoke, instant cache invalidation
- **Rate limiting** — Redis token bucket (burst 100, sustained 1000/min) per API key
- **Login brute-force protection** — Redis-based rate limiting (5 attempts/email/15min) with 429 + Retry-After
- **API request logging** — all proxy requests logged to partitioned `portal.api_logs` table
- **Audit logging** — key operations (create/rotate/revoke) logged to `portal.audit_logs`
- **Data proxy** — session-auth routes that mint short-lived JWTs (60s) to forward to Core
- **Portal SPA** — Dashboard, API Keys, Accounts, Transactions (with USD Value column), Reports, Org Settings

## Quick Start

### Docker (recommended)

```bash
make docker-up          # Build and start full stack (6 services)
make docker-e2e         # Run E2E tests (59 tests)
make docker-interactive # Interactive API testing console
make docker-down        # Stop (preserves data)
make docker-clean       # Stop + remove volumes (full reset)
```

Services started: PostgreSQL (:5432), Redis (:6379), migrations, bankie-core (:3030), bankie-gateway (:4040), portal-spa (:8080).

Portal UI available at `http://localhost:8080`.

### Local Development

```bash
make local-setup        # One-shot: infra + DB + build + JWT + core server
make local-gateway      # Start gateway server (background)
make local-portal       # Start SPA dev server (:5173, proxies to :4040)
make local-e2e          # Run E2E tests
make local-interactive  # Interactive API testing console
make local-stop         # Stop server + gateway + tear down infra
```

### Manual Setup

```bash
# Prerequisites: PostgreSQL 16, Redis
make db-pg-init-main    # Create DB user + database
make db-pg-migrate      # Run migrations

# Generate JWT
cargo run --bin bankie -- --mode secret_key              # Generate secret
cargo run --bin bankie -- --mode jwt --service {name}    # Generate tenant JWT

# Start core server (requires DB_PASSWD, JWT_SECRET env vars)
cargo run --bin bankie -- --mode server

# Start gateway (requires DB_PASSWD, JWT_SECRET, CORE_URL env vars)
cargo run --bin bankie-gateway
```

## API Endpoints

### Core (:3030)

All `/v1/*` endpoints require a JWT Bearer token.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Liveness check |
| GET | `/ready` | Readiness check (DB + Redis) |
| POST | `/v1/bank_account` | Execute command (open, approve, freeze, deposit, withdraw, transfer, close) |
| GET | `/v1/bank_account/:id` | Query bank account |
| GET | `/v1/bank_account/:id/sub-accounts` | List sub-accounts |
| GET | `/v1/bank_account/by-number/:account_number` | Lookup by account number |
| GET | `/v1/accounts?offset=&limit=` | List all accounts for tenant (paginated) |
| GET | `/v1/ledger/:id` | Query ledger balances |
| GET | `/v1/user/:id` | Query user's accounts with ledger |
| POST | `/v1/house_account` | Create house account |
| GET | `/v1/house_account?currency=` | List house accounts |
| GET | `/v1/transaction?bank_account_id=&offset=&limit=` | List transactions |
| GET | `/v1/bank_account/:id/balance-history` | Balance history from snapshots |
| GET | `/v1/report/settlement?start_date=&end_date=` | Settlement report CSV (max 90-day range) |

### Gateway (:4040) — Portal API

Portal routes use session cookie auth. External API proxy uses Bearer API key auth.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/portal/v1/auth/signup` | None | Create org + owner |
| POST | `/portal/v1/auth/login` | None | Login → session cookie |
| POST | `/portal/v1/auth/logout` | None | Clear cookies |
| GET | `/portal/v1/dashboard/stats` | Session | Org stats |
| GET | `/portal/v1/organization` | Session | Get org details |
| PUT | `/portal/v1/organization` | Session | Update org |
| GET | `/portal/v1/api-keys` | Session | List API keys |
| POST | `/portal/v1/api-keys` | Session | Create key (raw key shown once) |
| POST | `/portal/v1/api-keys/:id/rotate` | Session | Rotate key |
| DELETE | `/portal/v1/api-keys/:id` | Session | Revoke key |
| GET | `/portal/v1/data/*` | Session | Data proxy to Core |
| ANY | `/*` | API Key | Proxied to Core |

## Build & Test

```bash
# Build (use SQLX_OFFLINE=true when no live DB available)
SQLX_OFFLINE=true cargo build

# Unit tests (251 tests: 128 core + 114 gateway + 9 common, no DB needed)
SQLX_OFFLINE=true cargo test -- --nocapture
cargo test -p bankie-core test_name -- --nocapture      # Single test in crate
cargo test -p bankie-gateway test_name -- --nocapture

# Lint
cargo clippy --all-targets --tests --benches --no-default-features -- -D warnings
cargo fmt -- --check

# Coverage
cargo llvm-cov nextest

# Portal SPA
cd portal-spa && npm install && npm run dev     # Dev server on :5173
cd portal-spa && npm run build                  # Production build

# Concurrency pressure testing (requires k6 + running stack)
ACCOUNT_ID=<uuid> make over-withdrawn-test
```

## Configuration

| Config | Purpose |
|--------|---------|
| `config.{ENV}.yaml` | Core server config (DB, Redis, listen addr) |
| `config.gateway.{ENV}.yaml` | Gateway config (DB, Redis, core_url, JWT secret) |
| `SQLX_OFFLINE=true` | Build without live DB (uses cached queries) |

Environment variables: `DB_PASSWD`, `JWT_SECRET`, `CORE_URL`, `RUST_LOG`, `ENV` (defaults to `local`, set to `docker` in containers).

## Documentation

- [CLAUDE.md](CLAUDE.md) — Detailed architecture: CQRS/ES patterns, tenant isolation, middleware stacks, module layout
- [docs/dev-portal-prd.md](docs/dev-portal-prd.md) — Developer Portal PRD with phased roadmap
- [docs/dev-portal-tech-spec.md](docs/dev-portal-tech-spec.md) — Technical specification
- [docs/security-review.md](docs/security-review.md) — Security review findings
