# Bankie

Banking system implementing sub-account and ledger management with CQRS/Event Sourcing. Built in Rust using `cqrs-es` + `postgres-es`.

## Features

- **Multi-tenant** JWT-based authentication with tenant isolation
- **CQRS/Event Sourcing** with BankAccount and Ledger aggregates
- **Sub-accounts** — Checking (master), Interest, Yield linked via `parent_id`
- **Double-entry bookkeeping** — every transaction creates journal entries with debit/credit lines
- **Multi-asset support** — USD, TWD, BTC, ETH, USDT with currency-aware precision
- **Outbox pattern** — async ledger processing with Redis distributed locking
- **Idempotency** — Redis-backed `Idempotency-Key` header deduplication
- **Settlement reports** — CSV export with running balances and CSV-injection prevention
- **Balance snapshots** — daily snapshots for historical balance queries
- **Concurrency-safe** — bounded command channel (10k) with sequential processing

## Quick Start

### Docker (recommended)

```bash
make docker-up          # Start full stack (PostgreSQL + Redis + migrations + app)
make docker-e2e         # Run E2E tests (59 tests)
make docker-interactive # Interactive API testing console
make docker-down        # Stop (preserves data)
make docker-clean       # Stop + remove volumes (full reset)
```

### Local Development

```bash
make local-setup        # One-shot: infra + DB + build + JWT + server
make local-e2e          # Run E2E tests
make local-interactive  # Interactive API testing console
make local-stop         # Stop server + tear down infra
```

### Manual Setup

```bash
# Prerequisites: PostgreSQL 16, Redis
make db-pg-init-main    # Create DB user + database
make db-pg-migrate      # Run migrations

# Generate JWT secret and store in JWT_SECRET env var
cargo run --bin bankie -- --mode secret_key

# Generate JWT for a tenant service
cargo run --bin bankie -- --mode jwt --service {service_name}

# Start server (requires DB_PASSWD, JWT_SECRET env vars)
cargo run --bin bankie -- --mode server
```

## API Endpoints

All `/v1/*` endpoints require a JWT Bearer token.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Liveness check |
| GET | `/ready` | Readiness check (DB + Redis) |
| POST | `/v1/bank_account` | Execute bank account command (open, approve, freeze, deposit, withdraw, transfer, close) |
| GET | `/v1/bank_account/:id` | Query bank account |
| GET | `/v1/bank_account/:id/sub-accounts` | List sub-accounts under a master |
| GET | `/v1/bank_account/by-number/:account_number` | Lookup by account number |
| GET | `/v1/accounts?offset=&limit=` | List all accounts for tenant (paginated) |
| GET | `/v1/ledger/:id` | Query ledger balances |
| GET | `/v1/user/:id` | Query user's bank accounts with ledger |
| POST | `/v1/house_account` | Create house account |
| GET | `/v1/house_account?currency=` | List house accounts |
| GET | `/v1/transaction?bank_account_id=&offset=&limit=` | List transactions (filterable by date, type, status) |
| GET | `/v1/bank_account/:id/balance-history?start_date=&end_date=` | Balance history from snapshots |
| GET | `/v1/report/settlement?start_date=&end_date=` | Settlement report CSV (max 90-day range) |

## Build & Test

```bash
# Build (use SQLX_OFFLINE=true when no live DB available)
SQLX_OFFLINE=true cargo build

# Unit tests (126 tests, no DB needed)
SQLX_OFFLINE=true cargo test -- --nocapture

# Lint
cargo clippy --all-targets --tests --benches --no-default-features -- -D warnings
cargo fmt -- --check

# Coverage
cargo llvm-cov nextest

# Concurrency pressure testing (requires k6 + running server)
make over-withdrawn-test
```

## Configuration

- Config file: `config.{ENV}.yaml` (defaults to `config.local.yaml`)
- Environment variables: `DB_PASSWD`, `JWT_SECRET`, `RUST_LOG`, `ENV`
- Set `SQLX_OFFLINE=true` for builds without a live database
- Logging levels: `trace`, `debug`, `info`, `warn`, `error`

## Architecture

See [CLAUDE.md](CLAUDE.md) for detailed architecture documentation including CQRS/Event Sourcing patterns, tenant isolation, outbox processing, and module layout.
