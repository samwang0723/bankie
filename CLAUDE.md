# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Bankie is a banking/ledger system built in Rust implementing sub-account and ledger management using **CQRS/Event Sourcing** (via `cqrs-es` + `postgres-es`). It provides a REST API for managing bank accounts, ledgers, transactions, and house accounts with JWT-based multi-tenant authentication. Supports both fiat (USD, TWD) and crypto assets (BTC, ETH, USDT) with currency-aware precision.

## Build & Development Commands

```bash
# Build (use SQLX_OFFLINE=true when no live DB available)
SQLX_OFFLINE=true cargo build
cargo build --release --bin bankie

# Run tests (107 unit tests, no DB needed)
SQLX_OFFLINE=true cargo test -- --nocapture
cargo test test_name -- --nocapture      # Single test
cargo llvm-cov nextest                   # Coverage (requires cargo-nextest + cargo-llvm-cov)

# Lint (must pass CI — clippy treats warnings as errors)
cargo clippy --all-targets --tests --benches --no-default-features -- -D warnings
cargo fmt -- --check
cargo check --all

# Regenerate sqlx offline cache (REQUIRED after changing any SQL query)
DATABASE_URL="postgres://bankie_app:password@localhost:5432/bankie_main" cargo sqlx prepare

# E2E tests (39 tests, requires running stack)
make docker-up && make docker-e2e        # Docker full-stack
make local-setup && make local-e2e       # Local dev

# Docker lifecycle
make docker-up                           # Start full stack (postgres + redis + migrations + app)
make docker-down                         # Stop (preserves data)
make docker-clean                        # Stop + remove volumes (full data reset)

# Local dev lifecycle
make local-setup                         # One-shot: infra + db + build + jwt + server
make local-stop                          # Stop server + tear down infra

# Generate JWT
cargo run --bin bankie -- --mode secret_key              # Generate secret
cargo run --bin bankie -- --mode jwt --service {name}    # Generate tenant JWT

# Concurrency pressure testing (requires k6)
make over-withdrawn-test
```

## Infrastructure Dependencies

- **PostgreSQL 16** — event store, views, transactions, journals, outbox, balance snapshots
- **Redis** — distributed lock for outbox/snapshot jobs + idempotency key deduplication
- Config: `config.local.yaml` (loaded via `ENV` env var, defaults to `local`)
- Env vars: `DB_PASSWD`, `JWT_SECRET`, `RUST_LOG`, `SQLX_OFFLINE=true` (for offline builds)
- Docker Compose at root provides PostgreSQL + Redis + migrations + app container

## Architecture

### CQRS/Event Sourcing Core

Two aggregates using `cqrs-es`/`postgres-es`:

**BankAccount aggregate** (`src/event_sourcing/aggregate/bank_account.rs`)
- Commands: `OpenAccount`, `ApproveAccount`, `FreezeAccount`, `UnfreezeAccount`, `CloseAccount`, `Deposit`, `Withdrawal`, `Transfer`
- Events: `AccountOpened`, `AccountKycApproved`, `AccountFrozen`, `AccountUnfrozen`, `AccountClosed`, `CustomerDepositedCash`, `CustomerWithdrewCash`
- Commands sent via a **bounded** `mpsc` channel (capacity 10,000) and processed sequentially
- Deposit/Withdrawal create transactions + journal entries + outbox records (not aggregate events)
- Transfer creates paired debit/credit transactions between two accounts
- Account lifecycle: `Pending` → `Approved` → `Freeze`/`CustomerClosed`

**Ledger aggregate** (`src/event_sourcing/aggregate/ledger.rs`)
- Commands: `Init`, `Credit`, `DebitHold`, `DebitRelease`
- Events: `LedgerInitiated`, `LedgerUpdated`
- Tracks `available`, `pending`, and `current` balances using delta-based updates
- Withdrawal uses debit-hold pattern: moves funds from `available` to `pending` immediately, then releases via outbox job

### Tenant Isolation

Every entity carries `tenant_id`. The flow:
```
JWT claims → auth middleware → Extension<i32> → CommandExtractor.set_tenant_id()
  → Command.tenant_id → Aggregate → BaseEvent.tenant_id → View.update()
  → JSON payload (tenant_id field) → DB trigger → indexed tenant_id column
```

Key design decisions:
- **`#[serde(skip_deserializing)]`** on `tenant_id` in commands — clients cannot set it; only server-side injection from JWT
- **DB triggers** sync `tenant_id` from cqrs-es JSON `payload` column to indexed columns on `bank_account_views` and `ledger_views` (the cqrs-es framework only writes JSON, we can't directly set denormalized columns)
- **All SQL queries** filter by `AND tenant_id = $N`; all inserts include `tenant_id`
- Outbox records carry `tenant_id` so background jobs inherit correct tenant context

### Multi-Asset Support

- `Currency` enum (USD, TWD, BTC, ETH, USDT) with per-currency precision (2, 0, 8, 18, 6)
- `AssetRegistry` (`src/common/asset.rs`) — `Arc<RwLock<HashMap<String, Asset>>>` loaded at startup, validates asset codes at API boundary
- `Money` type carries `amount: Decimal` + `currency: Currency`, use `money.asset_code()` (not `.currency.to_string()`)
- Asset validation happens in route handlers before command dispatch

### Middleware Stack

Execution order (outermost → innermost → handler):
```
TraceLayer → CompressionLayer → AddExtension(State) → AddExtension(Redis)
  → authorize (JWT auth + tenant_id extraction into Extension<i32>)
  → idempotency_check (Redis SET NX EX, 24h TTL, tenant-scoped)
  → Handler
```

### Idempotency

`Idempotency-Key` HTTP header → Redis `SET NX EX 86400` scoped by `idempotency:{tenant_id}:{key}`. Duplicate requests return 409 Conflict. Fails open on Redis errors.

### Outbox Pattern + Cron Jobs

| Job | Schedule | Purpose |
|-----|----------|---------|
| Ledger job | Every 10s | Polls outbox → executes `LedgerCommand::Credit` or `DebitRelease`. Redis-locked. Retries with dead letter after 5 failures. |
| Balance snapshot | Daily midnight UTC | Snapshots all active account balances to `balance_snapshots` table. Redis-locked. |

### Key Data Flow

```
HTTP Request → JWT Auth (extracts tenant_id) → Idempotency Check → Route Handler
  → CommandExtractor (deserialize + generate IDs + validate amounts > 0 + inject tenant_id)
  → mpsc channel (bounded, 10k) → process_commands() → Aggregate::handle()
    → BankAccountServices (validates status/balance, creates transactions/journals/outbox with tenant_id)
    → Outbox cron job → Ledger aggregate (credit/debit release with tenant_id from outbox record)
```

### Double-Entry Bookkeeping

Every deposit/withdrawal creates a `Transaction` + `JournalEntry` + `JournalLine`s. Journal lines have debit/credit amounts linking user ledger and house account ledger.

### Structured Error Responses

`AppError` enum (`src/common/error.rs`) maps to HTTP status codes with JSON `{code, message}` body:
- `BadRequest(400)`, `Unauthorized(401)`, `Forbidden(403)`, `NotFound(404)`, `Conflict(409)`, `UnprocessableEntity(422)`, `InternalServerError(500)`

### Module Layout

- `src/auth/` — JWT generation, validation, Axum auth middleware (tenant-based)
- `src/command.rs` — `CommandExtractor` (validates amounts > 0, generates UUIDs/account numbers, injects tenant_id from JWT)
- `src/house_account.rs` — `HouseAccountExtractor`
- `src/common/money.rs` — `Money` type, `Currency` enum, precision functions
- `src/common/asset.rs` — `AssetRegistry`, `Asset`, `AssetClass` (Fiat/Crypto)
- `src/common/error.rs` — `AppError` structured HTTP error responses
- `src/common/idempotency.rs` — Idempotency-Key middleware (Redis-backed)
- `src/common/snowflake.rs` — Snowflake ID generation
- `src/common/account.rs` — Bank account number generation
- `src/configs/settings.rs` — Config loading from `config.{ENV}.yaml`
- `src/domain/` — Domain models, events, finance structs, tenant, user views
- `src/event_sourcing/` — Aggregates, commands, events, queries (CQRS view projections), helpers
- `src/repository/adapter.rs` — `DatabaseClient` trait + `Adapter` wrapper (`mockall` for testing)
- `src/repository/postgres.rs` — `DatabaseClient` impl for `PgPool` (all SQL queries with tenant filtering)
- `src/repository/redis.rs` — Redis lock + get/set operations
- `src/repository/configs.rs` — CQRS framework wiring
- `src/route.rs` — Axum route handlers (all pass tenant_id to queries)
- `src/service.rs` — `BankAccountApi` trait + `BankAccountLogic` (business logic)
- `src/state.rs` — `ApplicationState` (DB pool, Redis, CQRS loaders, command sender, AssetRegistry)
- `src/job.rs` — Outbox cron job + daily balance snapshot job

### API Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | No | Liveness check |
| GET | `/ready` | No | Readiness check (DB + Redis) |
| GET | `/v1/bank_account/:id` | Yes | Query bank account view |
| GET | `/v1/bank_account/:id/sub-accounts` | Yes | List sub-accounts under a master |
| GET | `/v1/bank_account/by-number/:account_number` | Yes | Lookup by account number |
| POST | `/v1/bank_account` | Yes | Execute bank account command |
| GET | `/v1/ledger/:id` | Yes | Query ledger view |
| GET | `/v1/house_account?currency=` | Yes | List house accounts |
| POST | `/v1/house_account` | Yes | Create house account |
| GET | `/v1/user/:id` | Yes | Query user's bank accounts with ledger |
| GET | `/v1/transaction?bank_account_id=&offset=&limit=` | Yes | List transactions (filterable by date, type, status) |
| GET | `/v1/bank_account/:id/balance-history?start_date=&end_date=` | Yes | Balance history from snapshots |

## Testing Patterns

- **107 unit tests** + **39 e2e tests**, unit tests need no DB (`SQLX_OFFLINE=true`)
- Aggregate tests use `cqrs_es::test::TestFramework` with given/when/then pattern and `test_case!`/`test_error_case!` macros
- `MockBankAccountServices` (manual mock in `bank_account.rs`) with configurable responses via `Mutex<Option<Result<...>>>` fields for negative-path testing
- DB layer uses `mockall::automock` on `DatabaseClient` trait
- Auth middleware tests use `MockDatabaseClient` + tower's `oneshot`
- `CommandExtractor` tests verify amount validation (zero/negative rejection) and tenant_id injection
- View projection tests cover all `BankAccountEvent` and `LedgerEvent` variants
- E2E tests (`scripts/e2e-test.sh`) cover the full banking lifecycle: house accounts, account opening/approval, deposit, withdrawal, transfer, freeze/unfreeze/close, query endpoints, and negative cases

## SQLx Offline Mode

Compile-time checked queries via `sqlx`. Set `SQLX_OFFLINE=true` and ensure `.sqlx/` directory has cached query metadata for builds without a live DB. **When any SQL query in `postgres.rs` changes**, you must regenerate the cache with a running DB:
```bash
DATABASE_URL="postgres://bankie_app:password@localhost:5432/bankie_main" cargo sqlx prepare
```

## Pre-commit Hooks

`.pre-commit-config.yaml`: `cargo fmt`, `cargo check`, `cargo clippy` (with `-D warnings`), `cargo test`.

## Two Binary Targets

- `bankie` (`src/main.rs`) — Main server with CLI modes: `server`, `secret_key`, `jwt`. Graceful shutdown via `SIGINT`.
- `migrations` (`src/repository/migrate.rs`) — Database migration runner
