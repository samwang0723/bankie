# Phase 3 Tech Spec: Webhooks & Observability

| Field | Value |
|-------|-------|
| **Owner Pillar** | Fiat / Onboarding |
| **Feature Label** | Developer Portal — Phase 3 |
| **Authors** | Architect Agent, sam.wang (SVP Eng) |
| **Audiences** | Engineering, Product, Security |
| **Status** | Draft |
| **Version** | 1.1 |
| **Reviewers** | — |
| **Dependencies** | Phase 2 complete (RBAC, rate limiter, grace expiry, member invite) — PR #19 |
| **Useful Links** | PRD: `docs/phase3-prd.md` · Tech Spec v2: `docs/dev-portal-tech-spec.md` |
| **Approved Date** | — |

---

## 1. TL;DR

Phase 3 adds a **webhook delivery system** to the Bankie Developer Portal. Tenants register HTTPS endpoints, subscribe to 6 banking event types, and receive HMAC-SHA256-signed payloads within seconds of state changes. The system uses a **DB-trigger-based event capture** from Core's CQRS event store into a `portal.webhook_events` staging table, followed by a **two-phase gateway dispatcher** (fan-out + delivery) with Redis-locked polling every 5 seconds. Failed deliveries retry with **exponential backoff** (7 attempts over ~17 hours). A **circuit breaker** auto-disables endpoints after 5 consecutive failures.

Additionally: **PII redaction** on `portal.api_logs` at write time, and two new SPA pages — **Webhooks** (endpoint CRUD + delivery history) and **Logs** (searchable API request log viewer).

**Impact**: Event-driven integrations replace polling. Reconciliation latency drops from minutes to seconds. Self-service log viewer eliminates DB access for debugging.

---

## 2. Architectural Decision: Event Capture Strategy (Q7)

### 2.1 The Problem

The PM asked (Q7): Should the dispatcher poll Core's `outbox` table directly (with a `processed_for_webhook` flag) or use a separate mechanism?

### 2.2 Analysis

The Core outbox contains **only ledger commands** (`LedgerCommand::Credit`, `LedgerCommand::DebitRelease`). These are internal CQRS operations, not the user-facing events the PRD specifies. The 6 required event types come from **two different sources**:

| Event Type | Source Table | Record Type |
|-----------|-------------|-------------|
| `account.opened` | `bank_account_events` | CQRS `BankAccountEvent::AccountOpened` |
| `account.approved` | `bank_account_events` | CQRS `BankAccountEvent::AccountKycApproved` |
| `account.frozen` | `bank_account_events` | CQRS `BankAccountEvent::AccountFrozen` |
| `account.closed` | `bank_account_events` | CQRS `BankAccountEvent::AccountClosed` |
| `transaction.completed` | `outbox` | `processed` flips `false → true` via `complete_transaction()` |
| `transaction.failed` | `outbox_dead_letter` | INSERT when `retry_count >= 5` |

**Polling the Core outbox alone cannot cover account lifecycle events** — those go through `bank_account_events` via cqrs-es and never touch the outbox table.

### 2.3 Recommendation: DB Triggers → `portal.webhook_events` Staging Table

```mermaid
graph LR
    subgraph "Core Tables (public schema)"
        BAE[bank_account_events]
        OB[outbox]
        DL[outbox_dead_letter]
    end

    subgraph "Triggers (portal schema)"
        T1[trg_capture_bank_account_event<br/>AFTER INSERT on bank_account_events]
        T2[trg_capture_transaction_completed<br/>AFTER UPDATE on outbox<br/>WHERE processed = true]
        T3[trg_capture_transaction_failed<br/>AFTER INSERT on outbox_dead_letter]
    end

    subgraph "Staging"
        WE[portal.webhook_events<br/>unified staging table]
    end

    BAE --> T1 --> WE
    OB --> T2 --> WE
    DL --> T3 --> WE

    style T1 fill:#f9f,stroke:#333
    style T2 fill:#f9f,stroke:#333
    style T3 fill:#f9f,stroke:#333
```

**Why this over `processed_for_webhook` flag on outbox:**

| Factor | Direct outbox poll + flag | DB triggers → staging table |
|--------|--------------------------|---------------------------|
| Core code changes | Must add `processed_for_webhook` column to outbox | **None** — triggers are in `portal` schema functions |
| Event coverage | Only 2 of 6 events (transaction only) | **All 6** — triggers on 3 source tables |
| Schema coupling | Gateway reads Core's outbox format | Gateway reads its own `portal.webhook_events` format |
| Event name mapping | Gateway must understand Core's `LedgerCommand::Credit` → `transaction.completed` | **Trigger does the mapping** — inserts clean event type names |
| Ledger job interference | Adds WHERE clause complexity to outbox polling | **Zero interference** — outbox cron is untouched |
| Rollback safety | Removing flag requires Core migration | Drop triggers + table — portal-only cleanup |

**Verdict**: DB triggers + staging table. Zero Core changes, covers all 6 event types, clean separation.

---

## 3. System Design

### 3.1 End-to-End Webhook Flow

```mermaid
sequenceDiagram
    participant Client as External Client
    participant Core as Bankie Core :3030
    participant PG as PostgreSQL
    participant Trigger as DB Triggers
    participant WES as portal.webhook_events
    participant Fanout as Fan-out Job (5s)
    participant Redis as Redis
    participant Deliver as Delivery Job (5s)
    participant Endpoint as Tenant Endpoint

    Client->>Core: POST /v1/bank_account (Deposit)
    Core->>PG: INSERT bank_account_events (CustomerDepositedCash)
    PG->>Trigger: AFTER INSERT trigger
    Trigger->>WES: INSERT (event_type='account.deposited' — filtered out, not in scope)

    Note over Core,PG: Later: outbox cron processes deposit
    Core->>PG: UPDATE outbox SET processed=true
    PG->>Trigger: AFTER UPDATE trigger (processed=true)
    Trigger->>WES: INSERT (event_type='transaction.completed', payload with tx details)

    rect rgb(230, 240, 255)
        Note over Fanout,WES: Fan-out cycle (every 5s)
        Fanout->>Redis: SET NX gw:lock:webhook_fanout
        Fanout->>PG: SELECT FROM portal.webhook_events WHERE processed=false LIMIT 100
        Fanout->>PG: SELECT FROM portal.webhook_endpoints WHERE tenant_id matches AND event_type subscribed
        Fanout->>PG: INSERT portal.webhook_deliveries (status='pending')
        Fanout->>PG: UPDATE portal.webhook_events SET processed=true
        Fanout->>Redis: DEL gw:lock:webhook_fanout
    end

    rect rgb(230, 255, 230)
        Note over Deliver,Endpoint: Delivery cycle (every 5s)
        Deliver->>Redis: SET NX gw:lock:webhook_deliver
        Deliver->>PG: SELECT pending deliveries WHERE next_retry_at <= now()
        Deliver->>Deliver: Sign payload (HMAC-SHA256)
        Deliver->>Endpoint: POST (signed payload)

        alt 2xx
            Deliver->>PG: UPDATE delivery status='success'
            Deliver->>PG: UPDATE endpoint failure_count=0
        else non-2xx / timeout
            Deliver->>PG: UPDATE delivery attempt++, next_retry_at (backoff)
            Deliver->>PG: UPDATE endpoint failure_count++
            alt failure_count >= 5
                Deliver->>PG: UPDATE endpoint status='disabled'
            end
            alt attempt > 7
                Deliver->>PG: UPDATE delivery status='dead_letter'
            end
        end
        Deliver->>Redis: DEL gw:lock:webhook_deliver
    end
```

### 3.2 Architecture Position

```mermaid
graph TB
    subgraph "Bankie Core :3030 (unchanged)"
        CQRS[CQRS Aggregates]
        BAE[bank_account_events]
        OB[outbox + outbox_dead_letter]
    end

    subgraph "PostgreSQL Triggers"
        T1[trg_capture_bank_account_event]
        T2[trg_capture_transaction_completed]
        T3[trg_capture_transaction_failed]
    end

    subgraph "portal schema"
        WES[portal.webhook_events<br/>staging]
        WEP[portal.webhook_endpoints]
        WD[portal.webhook_deliveries]
    end

    subgraph "bankie-gateway :4040"
        subgraph "Portal API"
            WH_ROUTES[Webhook Routes<br/>CRUD + delivery logs]
            LOG_ROUTES[Log Routes<br/>API log viewer]
        end
        subgraph "Background Jobs"
            GRACE[Grace Expiry<br/>every 60s]
            FAN[Webhook Fan-out<br/>every 5s]
            DELIVER[Webhook Delivery<br/>every 5s]
        end
    end

    subgraph "External"
        EP[Tenant HTTPS Endpoints]
    end

    CQRS --> BAE & OB
    BAE --> T1 --> WES
    OB --> T2 --> WES
    OB --> T3 --> WES
    FAN -->|poll unprocessed| WES
    FAN -->|match subscriptions| WEP
    FAN -->|create deliveries| WD
    DELIVER -->|poll pending| WD
    DELIVER -->|POST signed| EP
    WH_ROUTES --> WEP & WD
```

### 3.3 Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **DB triggers for event capture** | Zero Core code changes. 3 triggers (on `bank_account_events` INSERT, `outbox` UPDATE, `outbox_dead_letter` INSERT) cover all 6 event types. Triggers map Core's internal event names to PRD-specified names. |
| **Two-phase dispatch** | Fan-out (DB-only, fast) separated from delivery (network I/O, slow). One slow endpoint cannot delay other deliveries. |
| **Gateway-side dispatcher** | Reuses `tokio::spawn` + `interval` + Redis lock pattern from grace expiry job. No new binary. |
| **5-second polling** | At 1000 tenants × 10 events/min = ~83 events/cycle — well within capacity. |
| **Build in-house** | Tables exist, patterns proven, <100 tenants, $0 incremental cost. Revisit Svix at >1000 tenants. |
| **Stripe-compatible signature format** | `t=<ts>,v1=<sig>` in `X-Bankie-Signature` — industry standard, familiar to integrators. |
| **Event dedup via `event_source_id`** | Unique constraint `(endpoint_id, event_source_id)` on `webhook_deliveries` prevents duplicate fan-out. |

---

## 4. Webhook Event Types

### 4.1 Phase 3 Scope (6 Events)

| Event Type | Core Source | Core Event | Trigger Source |
|-----------|-----------|------------|---------------|
| `account.opened` | `bank_account_events` | `BankAccountEvent::AccountOpened` | `trg_capture_bank_account_event` |
| `account.approved` | `bank_account_events` | `BankAccountEvent::AccountKycApproved` | `trg_capture_bank_account_event` |
| `account.frozen` | `bank_account_events` | `BankAccountEvent::AccountFrozen` | `trg_capture_bank_account_event` |
| `account.closed` | `bank_account_events` | `BankAccountEvent::AccountClosed` | `trg_capture_bank_account_event` |
| `transaction.completed` | `outbox` | `processed` → `true` | `trg_capture_transaction_completed` |
| `transaction.failed` | `outbox_dead_letter` | INSERT (max retries) | `trg_capture_transaction_failed` |

### 4.2 Event Name Mapping (Core → Webhook)

The DB trigger maps Core's internal event names to the PRD's simplified names:

```sql
CASE NEW.event_type
    WHEN 'bank_account.opened'       THEN 'account.opened'
    WHEN 'bank_account.kyc_approved' THEN 'account.approved'
    WHEN 'bank_account.frozen'       THEN 'account.frozen'
    WHEN 'bank_account.closed'       THEN 'account.closed'
    ELSE NULL  -- skip unmapped events (e.g., deposited, withdrew, unfrozen)
END
```

### 4.3 Payload Envelope

All webhook payloads follow a consistent envelope per the PRD:

```json
{
  "id": "evt_550e8400-e29b-41d4-a716-446655440000",
  "type": "account.approved",
  "created_at": "2026-03-02T14:30:00Z",
  "tenant_id": 100,
  "data": {
    "account_id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "approved",
    "account_type": "checking",
    "currency": "USD"
  }
}
```

### 4.4 Rust Types

```rust
/// Webhook event envelope sent to tenant endpoints
#[derive(Debug, Clone, Serialize)]
pub struct WebhookPayload {
    pub id: String,                  // "evt_" + Uuid
    #[serde(rename = "type")]
    pub event_type: String,          // e.g., "account.approved"
    pub created_at: DateTime<Utc>,
    pub tenant_id: i32,
    pub data: serde_json::Value,     // event-specific data
}

/// Staging table row — populated by DB triggers
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WebhookEvent {
    pub id: i64,
    pub tenant_id: i32,
    pub event_type: String,          // PRD name: "account.opened", etc.
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub source_id: String,           // unique event source identifier for dedup
    pub payload: serde_json::Value,  // cleaned payload for tenant consumption
    pub processed: bool,
    pub created_at: DateTime<Utc>,
}
```

---

## 5. HMAC-SHA256 Signing & Verification

### 5.1 Signature Format (Stripe-Compatible)

Single `X-Bankie-Signature` header combining timestamp and signature:

```
X-Bankie-Signature: t=1709366400,v1=5d3a1f2b...
```

| Component | Description |
|-----------|-------------|
| `t=` | Unix timestamp (seconds) — when signature was generated |
| `v1=` | Hex-encoded HMAC-SHA256 of `{timestamp}.{raw_json_body}` |

Additional headers for debugging:

| Header | Example | Purpose |
|--------|---------|---------|
| `X-Bankie-Event-Id` | `evt_550e8400...` | Idempotency key for dedup |
| `X-Bankie-Webhook-Id` | `dlv_d4e5f6...` | Delivery ID for tracking |
| `Content-Type` | `application/json` | Always JSON |
| `User-Agent` | `Bankie-Webhooks/1.0` | Source identification |

### 5.2 Signature Construction

```rust
pub fn generate_signing_secret() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    format!("whsec_{}", hex::encode(bytes))
}

pub fn sign_payload(secret: &str, timestamp: i64, body: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let signed_content = format!("{}.{}", timestamp, body);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(signed_content.as_bytes());
    let result = mac.finalize();
    format!("t={},v1={}", timestamp, hex::encode(result.into_bytes()))
}
```

### 5.3 Verification (Tenant-Side)

Tenants verify by:
1. Parse `t=` and `v1=` from `X-Bankie-Signature`
2. Reject if `abs(now() - t) > 300` seconds (replay protection)
3. Compute `expected = HMAC-SHA256(signing_secret, "{t}.{body}")`
4. Constant-time compare `expected` with `v1`

Python example in PRD Appendix B.

### 5.4 Signing Secret Lifecycle

- Generated per endpoint at creation: `whsec_` + 64 hex chars
- Returned **once** at creation (same pattern as API key `raw_key`)
- Subsequent GET responses show masked prefix: `whsec_a1b2****`
- Rotation via `POST /webhooks/:id/rotate-secret`: new secret generated, old secret kept for 24h grace. During grace, deliveries include both signatures: `t=...,v1=<new>,v1old=<old>`

---

## 6. Retry Strategy

### 6.1 Exponential Backoff Schedule (Per PRD)

| Attempt | Delay After Failure | Cumulative Time |
|---------|-------------------|-----------------|
| 1 | Immediate | 0 |
| 2 | 30 seconds | 30s |
| 3 | 2 minutes | 2m 30s |
| 4 | 15 minutes | 17m 30s |
| 5 | 1 hour | 1h 17m |
| 6 | 4 hours | 5h 17m |
| 7 | 12 hours | 17h 17m |
| Dead letter | — | After attempt 7 |

```rust
const MAX_ATTEMPTS: i32 = 7;

/// Explicit retry delays matching PRD schedule
const RETRY_DELAYS_SECS: &[u64] = &[
    30,      // attempt 2
    120,     // attempt 3: 2 minutes
    900,     // attempt 4: 15 minutes
    3_600,   // attempt 5: 1 hour
    14_400,  // attempt 6: 4 hours
    43_200,  // attempt 7: 12 hours
];

fn next_retry_at(attempt: i32) -> Option<DateTime<Utc>> {
    if attempt >= MAX_ATTEMPTS {
        return None; // → dead_letter
    }
    let idx = (attempt - 1) as usize;
    let base_delay = RETRY_DELAYS_SECS.get(idx).copied().unwrap_or(43_200);
    // Jitter: ±10%
    let jitter_range = base_delay / 10;
    let jitter = rand::thread_rng().gen_range(0..=jitter_range);
    Some(Utc::now() + chrono::Duration::seconds((base_delay + jitter) as i64))
}
```

### 6.2 Circuit Breaker (Per PRD)

Per-endpoint based on `failure_count` in `portal.webhook_endpoints`:

| failure_count | Action |
|---------------|--------|
| 0–4 | Normal delivery |
| ≥ 5 | Auto-disable endpoint (`status = 'disabled'`, `disabled_at = now()`) |

On successful delivery → `failure_count` resets to 0.
On manual re-enable via API → `failure_count` resets to 0.

### 6.3 Dead Letter

- Deliveries exhausting all 7 attempts → `status = 'dead_letter'`
- Retained in `webhook_deliveries` for 30 days
- Manual retry via `POST /webhooks/:id/deliveries/:id/retry` resets `attempt_number = 1` (fresh attempt count)
- Not automatically retried

---

## 7. Webhook Dispatcher Architecture

### 7.1 Module Structure

```
crates/bankie-gateway/src/
├── job.rs                        # + spawn_webhook_fanout_job()
│                                 # + spawn_webhook_delivery_job()
├── webhook/
│   ├── mod.rs                    # pub use
│   ├── dispatcher.rs             # Fan-out: webhook_events → deliveries
│   ├── deliverer.rs              # HTTP delivery + signing + retry
│   └── signing.rs                # HMAC-SHA256 + secret generation
├── models/
│   └── webhook.rs                # WebhookEndpoint, WebhookDelivery, enums, DTOs
├── repo/
│   └── webhook.rs                # WebhookRepository trait + PgWebhookRepository
├── routes/
│   ├── webhook.rs                # Endpoint CRUD + delivery logs
│   └── log.rs                    # API logs viewer
└── redaction.rs                  # PII redaction utilities
```

### 7.2 Fan-Out Job (`dispatcher.rs`)

Runs every 5s. Redis lock: `gw:lock:webhook_fanout` (TTL 30s).

```rust
pub async fn run_fanout_cycle(state: &PortalState) {
    // 1. Acquire Redis lock
    // 2. SELECT FROM portal.webhook_events WHERE processed = false ORDER BY id LIMIT 100
    // 3. For each event:
    //    a. Resolve org_id from tenant_id via organizations table
    //    b. Find matching active endpoints:
    //       SELECT FROM portal.webhook_endpoints
    //       WHERE org_id = $1 AND status = 'active'
    //       AND event_types @> to_jsonb($2::text)
    //    c. For each matching endpoint:
    //       INSERT INTO portal.webhook_deliveries (
    //         id, endpoint_id, event_type, payload,
    //         event_source_id,   -- for dedup unique constraint
    //         status='pending', attempt_number=1, next_retry_at=now()
    //       ) ON CONFLICT (endpoint_id, event_source_id) DO NOTHING
    // 4. UPDATE portal.webhook_events SET processed = true WHERE id IN (...)
    // 5. Release lock
}
```

### 7.3 Delivery Job (`deliverer.rs`)

Runs every 5s. Redis lock: `gw:lock:webhook_deliver` (TTL 60s).

```rust
pub async fn run_delivery_cycle(state: &PortalState) {
    // 1. Acquire Redis lock
    // 2. SELECT d.*, e.url, e.signing_secret, e.failure_count
    //    FROM portal.webhook_deliveries d
    //    JOIN portal.webhook_endpoints e ON d.endpoint_id = e.id
    //    WHERE d.status = 'pending' AND d.next_retry_at <= now()
    //    AND e.status = 'active'
    //    ORDER BY d.next_retry_at LIMIT 50
    // 3. Spawn up to 10 concurrent deliveries via tokio::JoinSet:
    //    a. Serialize payload → JSON body
    //    b. Sign: X-Bankie-Signature = sign_payload(secret, now_unix, body)
    //    c. POST to endpoint.url with headers (30s timeout, no redirects)
    //    d. On 2xx: mark success, reset endpoint failure_count
    //    e. On non-2xx/timeout: increment attempt, set next_retry_at,
    //       increment failure_count, check circuit breaker threshold
    // 4. Release lock
}

lazy_static! {
    static ref WEBHOOK_CLIENT: reqwest::Client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Bankie-Webhooks/1.0")
        .build()
        .expect("Failed to build webhook HTTP client");
}
```

### 7.4 Job Registration in `main.rs`

```rust
// After PortalState construction:
job::spawn_grace_expiry_job(portal_state.clone());     // existing (60s)
job::spawn_webhook_fanout_job(portal_state.clone());   // new (5s)
job::spawn_webhook_delivery_job(portal_state.clone()); // new (5s)
```

---

## 8. API Design

### 8.1 Webhook Endpoint CRUD

| Method | Path | Auth | RBAC | Description |
|--------|------|------|------|-------------|
| POST | `/portal/v1/webhooks` | Session | Owner/Admin | Register endpoint |
| GET | `/portal/v1/webhooks` | Session | Any | List org's endpoints |
| GET | `/portal/v1/webhooks/:id` | Session | Any | Get endpoint detail |
| PATCH | `/portal/v1/webhooks/:id` | Session | Owner/Admin | Update URL, event types, status |
| DELETE | `/portal/v1/webhooks/:id` | Session | Owner/Admin | Delete endpoint |
| POST | `/portal/v1/webhooks/:id/rotate-secret` | Session | Owner/Admin | Rotate signing secret (24h grace) |
| POST | `/portal/v1/webhooks/:id/test` | Session | Owner/Admin | Send test ping (P2 — COULD) |

### 8.2 Webhook Delivery Logs

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/portal/v1/webhooks/:id/deliveries` | Session | List deliveries (paginated, filterable by status) |
| GET | `/portal/v1/webhooks/:id/deliveries/:delivery_id` | Session | Get delivery detail |
| POST | `/portal/v1/webhooks/:id/deliveries/:delivery_id/retry` | Session (Owner/Admin) | Retry dead-letter (P2 — COULD) |

### 8.3 API Logs Viewer

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/portal/v1/logs` | Session | Paginated + filtered API request logs |

Query params: `?page=1&per_page=50&method=POST&status_code=500&path=/v1/bank_account&from=...&to=...`

### 8.4 Request/Response Shapes

#### Create Webhook Endpoint

```json
// POST /portal/v1/webhooks
{
  "url": "https://api.acme.com/webhooks/bankie",
  "event_types": ["account.approved", "transaction.completed"],
  "description": "Production transaction webhook"
}

// Response: 201 Created
{
  "id": "uuid",
  "url": "https://api.acme.com/webhooks/bankie",
  "signing_secret": "whsec_a1b2c3d4...",
  "event_types": ["account.approved", "transaction.completed"],
  "description": "Production transaction webhook",
  "status": "active",
  "failure_count": 0,
  "created_at": "2026-03-02T10:00:00Z"
}
```

> `signing_secret` returned **once** at creation. Subsequent GETs show `whsec_a1b2****`.

#### List Deliveries

```json
// GET /portal/v1/webhooks/:id/deliveries?page=1&per_page=20&status=failed
{
  "deliveries": [
    {
      "id": "uuid",
      "event_type": "transaction.completed",
      "event_source_id": "ba_evt_550e8400..._5",
      "status": "success",
      "http_status": 200,
      "attempt_number": 1,
      "latency_ms": 245,
      "created_at": "2026-03-02T10:00:00Z"
    }
  ],
  "total": 42,
  "page": 1,
  "per_page": 20
}
```

#### API Logs

```json
// GET /portal/v1/logs?page=1&per_page=50&method=POST
{
  "logs": [
    {
      "id": 12345,
      "method": "POST",
      "path": "/v1/bank_account",
      "status_code": 200,
      "latency_ms": 45,
      "client_ip": "203.0.113.***",
      "created_at": "2026-03-02T10:00:00Z"
    }
  ],
  "total": 1523,
  "page": 1,
  "per_page": 50
}
```

### 8.5 Route Registration

```rust
// routes/webhook.rs
pub fn webhook_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/webhooks", post(create_endpoint).get(list_endpoints))
        .route("/webhooks/:id",
            get(get_endpoint).patch(update_endpoint).delete(delete_endpoint))
        .route("/webhooks/:id/rotate-secret", post(rotate_secret))
        .route("/webhooks/:id/test", post(send_test_event))
        .route("/webhooks/:id/deliveries", get(list_deliveries))
        .route("/webhooks/:id/deliveries/:delivery_id", get(get_delivery))
        .route("/webhooks/:id/deliveries/:delivery_id/retry", post(retry_delivery))
}

// routes/log.rs
pub fn log_routes() -> Router<Arc<PortalState>> {
    Router::new()
        .route("/logs", get(list_api_logs))
}

// In routes/mod.rs — add to protected block:
.merge(webhook::webhook_routes())
.merge(log::log_routes())
```

---

## 9. Rust Type Definitions

### 9.1 Models (`models/webhook.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EndpointStatus { Active, Disabled }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus { Pending, Success, Failed, DeadLetter }

/// Allowed webhook event types (validated at endpoint creation)
pub const VALID_EVENT_TYPES: &[&str] = &[
    "account.opened",
    "account.approved",
    "account.frozen",
    "account.closed",
    "transaction.completed",
    "transaction.failed",
];

#[derive(Debug, Clone, Serialize)]
pub struct WebhookEndpoint {
    pub id: Uuid,
    pub org_id: Uuid,
    pub url: String,
    #[serde(skip_serializing)]
    pub signing_secret: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub failure_count: i32,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub endpoint_id: Uuid,
    pub event_type: String,
    pub event_source_id: String,
    pub payload: serde_json::Value,
    pub http_status: Option<i32>,
    pub attempt_number: i32,
    pub status: DeliveryStatus,
    pub response_body: Option<String>,
    pub latency_ms: Option<i32>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// --- Request DTOs ---

#[derive(Debug, Deserialize)]
pub struct CreateEndpointRequest {
    pub url: String,
    pub event_types: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEndpointRequest {
    pub url: Option<String>,
    pub event_types: Option<Vec<String>>,
    pub description: Option<String>,
    pub status: Option<EndpointStatus>,
}

// --- Response DTOs ---

#[derive(Debug, Serialize)]
pub struct CreateEndpointResponse {
    pub id: Uuid,
    pub url: String,
    pub signing_secret: String,  // shown once
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct EndpointListItem {
    pub id: Uuid,
    pub url: String,
    pub signing_secret_prefix: String,  // "whsec_a1b2****"
    pub event_types: Vec<String>,
    pub description: Option<String>,
    pub status: EndpointStatus,
    pub failure_count: i32,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct DeliveryListItem {
    pub id: Uuid,
    pub event_type: String,
    pub event_source_id: String,
    pub status: DeliveryStatus,
    pub http_status: Option<i32>,
    pub attempt_number: i32,
    pub latency_ms: Option<i32>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RotateSecretResponse {
    pub new_signing_secret: String,
    pub grace_expires_at: DateTime<Utc>,
}
```

### 9.2 Repository Trait (`repo/webhook.rs`)

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WebhookRepository: Send + Sync {
    // Endpoint CRUD
    async fn create_endpoint(&self, id: Uuid, org_id: Uuid, url: String,
        signing_secret: String, event_types: Vec<String>,
        description: Option<String>) -> Result<WebhookEndpoint, RepoError>;
    async fn find_endpoint_by_id(&self, id: Uuid, org_id: Uuid)
        -> Result<Option<WebhookEndpoint>, RepoError>;
    async fn list_endpoints_by_org(&self, org_id: Uuid)
        -> Result<Vec<WebhookEndpoint>, RepoError>;
    async fn update_endpoint(&self, id: Uuid, org_id: Uuid,
        req: UpdateEndpointRequest) -> Result<Option<WebhookEndpoint>, RepoError>;
    async fn delete_endpoint(&self, id: Uuid, org_id: Uuid) -> Result<bool, RepoError>;

    // Secret rotation
    async fn rotate_signing_secret(&self, id: Uuid, org_id: Uuid,
        new_secret: String, grace_expires_at: DateTime<Utc>)
        -> Result<Option<WebhookEndpoint>, RepoError>;

    // Circuit breaker (used by delivery job)
    async fn increment_failure_count(&self, id: Uuid) -> Result<i32, RepoError>;
    async fn reset_failure_count(&self, id: Uuid) -> Result<(), RepoError>;
    async fn disable_endpoint(&self, id: Uuid) -> Result<(), RepoError>;

    // Fan-out queries
    async fn find_active_endpoints_for_tenant(&self, tenant_id: i32,
        event_type: &str) -> Result<Vec<WebhookEndpoint>, RepoError>;

    // Webhook events (staging table)
    async fn list_unprocessed_events(&self, limit: i64)
        -> Result<Vec<WebhookEvent>, RepoError>;
    async fn mark_events_processed(&self, ids: &[i64]) -> Result<(), RepoError>;

    // Deliveries
    async fn create_delivery(&self, id: Uuid, endpoint_id: Uuid,
        event_type: String, event_source_id: String,
        payload: serde_json::Value) -> Result<WebhookDelivery, RepoError>;
    async fn list_pending_deliveries(&self, limit: i64)
        -> Result<Vec<PendingDelivery>, RepoError>;
    async fn update_delivery_success(&self, id: Uuid,
        http_status: i32, latency_ms: i32) -> Result<(), RepoError>;
    async fn update_delivery_failure(&self, id: Uuid,
        http_status: Option<i32>, latency_ms: Option<i32>,
        response_body: Option<String>,
        next_retry_at: Option<DateTime<Utc>>) -> Result<(), RepoError>;
    async fn move_to_dead_letter(&self, id: Uuid) -> Result<(), RepoError>;
    async fn list_deliveries_by_endpoint(&self, endpoint_id: Uuid,
        page: i64, per_page: i64, status_filter: Option<String>)
        -> Result<(Vec<WebhookDelivery>, i64), RepoError>;
    async fn find_delivery_by_id(&self, id: Uuid, endpoint_id: Uuid)
        -> Result<Option<WebhookDelivery>, RepoError>;
    async fn reset_delivery_for_retry(&self, id: Uuid)
        -> Result<Option<WebhookDelivery>, RepoError>;

    // API logs (for viewer)
    async fn list_api_logs(&self, org_id: Uuid, filters: ApiLogFilters,
        page: i64, per_page: i64) -> Result<(Vec<ApiLogEntry>, i64), RepoError>;
}

/// Joined struct for delivery job (avoids N+1)
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    pub delivery: WebhookDelivery,
    pub endpoint_url: String,
    pub signing_secret: String,
    pub old_signing_secret: Option<String>,
    pub secret_grace_expires_at: Option<DateTime<Utc>>,
    pub endpoint_failure_count: i32,
}

#[derive(Debug)]
pub struct ApiLogFilters {
    pub method: Option<String>,
    pub status_code: Option<i32>,
    pub path: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}
```

### 9.3 PortalState Extension

```rust
// state.rs
pub struct PortalState {
    pub org_repo: Arc<dyn OrgRepository>,
    pub member_repo: Arc<dyn MemberRepository>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub dashboard_repo: Arc<dyn DashboardRepository>,
    pub webhook_repo: Arc<dyn WebhookRepository>,  // NEW
    pub jwt_secret: String,
    pub redis_client: Option<redis::Client>,
}
```

---

## 10. PII Redaction Strategy

### 10.1 Redaction at Write Time (in `api_logger` middleware)

| Field | Redaction Rule |
|-------|---------------|
| `client_ip` | IPv4: mask last octet (`203.0.113.***`). IPv6: mask last 64 bits. |
| `request_summary.authorization` | Replace with `[REDACTED]` |
| `request_summary.password` | Replace with `[REDACTED]` |
| `response_summary.token` | Replace with `[REDACTED]` |
| `response_summary.raw_key` | Replace with `[REDACTED]` |
| `response_summary.signing_secret` | Replace with `[REDACTED]` |
| `path` | Strip query params matching `token`, `secret`, `password` |

### 10.2 Implementation

```rust
// redaction.rs

const SENSITIVE_KEYS: &[&str] = &[
    "password", "password_hash", "authorization", "token",
    "raw_key", "signing_secret", "secret", "credential",
];

pub fn redact_pii(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if SENSITIVE_KEYS.contains(&key.to_lowercase().as_str()) {
                    *val = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_pii(val);
                }
            }
        }
        serde_json::Value::Array(arr) => arr.iter_mut().for_each(redact_pii),
        _ => {}
    }
}

pub fn mask_ipv4(ip: &str) -> String {
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            format!("{}.{}.{}.***", o[0], o[1], o[2])
        }
        Ok(std::net::IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}:****:****:****:****",
                s[0], s[1], s[2], s[3])
        }
        Err(_) => "***".to_string(),
    }
}
```

### 10.3 Webhook Payloads

Webhook payloads contain **no PII** — only IDs, statuses, amounts, timestamps. The `response_body` from tenant endpoints is truncated to 1KB and stored as-is (opaque, we don't control its format).

---

## 11. SPA Pages

### 11.1 Webhooks Page

```
┌─────────────────────────────────────────────────────┐
│  Webhooks                            [+ New Endpoint]│
├─────────────────────────────────────────────────────┤
│  ┌─────────────────────────────────────────────────┐│
│  │ https://api.acme.com/webhooks/bankie            ││
│  │ Events: account.approved, transaction.completed ││
│  │ Status: ● Active  │  Failures: 0               ││
│  │ Created: Mar 1, 2026                            ││
│  │                     [Deliveries] [Edit] [Delete]││
│  └─────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────┐│
│  │ https://hooks.slack.com/services/T.../B...      ││
│  │ Events: account.opened                          ││
│  │ Status: ○ Disabled (auto — 5 consecutive fails) ││
│  │                 [Deliveries] [Re-enable] [Delete]││
│  └─────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────┘
```

### 11.2 Delivery History (per endpoint)

```
┌──────────────────────────────────────────────────────┐
│  Deliveries: https://api.acme.com/webhooks/bankie    │
│  Filter: [All ▾]                                     │
├──────┬───────────────────────┬────────┬──────┬───────┤
│ Time │ Event                 │ Status │ Code │ Retry │
├──────┼───────────────────────┼────────┼──────┼───────┤
│ 10:30│ transaction.completed │   ✓   │  200 │  1/7  │
│ 10:28│ account.approved      │   ✗   │  503 │  3/7  │
│ 10:20│ transaction.completed │   ☠   │  —   │  7/7  │
├──────┴───────────────────────┴────────┴──────┴───────┤
│  Page 1 of 3                     [← Prev] [Next →]  │
└──────────────────────────────────────────────────────┘
```

### 11.3 API Logs Page

```
┌───────────────────────────────────────────────────────┐
│  API Logs                                             │
│  [Method ▾] [Status ▾] [Path ________] [Date range ▾]│
├──────┬────────┬──────────────────┬──────┬──────┬──────┤
│ Time │ Method │ Path             │ Code │ ms   │ IP   │
├──────┼────────┼──────────────────┼──────┼──────┼──────┤
│ 10:31│ POST   │ /v1/bank_account │  200 │   45 │203.**│
│ 10:30│ GET    │ /v1/accounts     │  200 │   12 │203.**│
│ 10:29│ POST   │ /v1/bank_account │  422 │    8 │ 10.**│
├──────┴────────┴──────────────────┴──────┴──────┴──────┤
│  Page 1 of 31                    [← Prev] [Next →]   │
└───────────────────────────────────────────────────────┘
```

### 11.4 Dashboard Extension

Add to existing `GET /portal/v1/dashboard/stats`:

```json
{
  "webhook_stats": {
    "total_endpoints": 3,
    "active_endpoints": 2,
    "deliveries_24h": 156,
    "success_rate_24h": 0.95
  }
}
```

---

## 12. Migration Plan

### 12.1 Migration 1: `portal.webhook_events` Staging Table

```sql
-- 20260310000001_webhook_events_staging.sql

CREATE TABLE portal.webhook_events (
    id BIGSERIAL PRIMARY KEY,
    tenant_id INT NOT NULL,
    event_type TEXT NOT NULL,           -- PRD name: "account.opened", etc.
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    source_id TEXT NOT NULL,            -- unique event source ID for dedup
    payload JSONB NOT NULL,             -- cleaned payload for tenant
    processed BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_webhook_events_unprocessed
    ON portal.webhook_events (created_at)
    WHERE processed = false;

CREATE INDEX idx_webhook_events_cleanup
    ON portal.webhook_events (created_at)
    WHERE processed = true;
```

### 12.2 Migration 2: DB Triggers for Event Capture

```sql
-- 20260310000002_webhook_event_triggers.sql

-- Trigger 1: Capture account lifecycle events from bank_account_events
CREATE OR REPLACE FUNCTION portal.capture_bank_account_event()
RETURNS TRIGGER AS $$
DECLARE
    v_tenant_id INT;
    v_webhook_event_type TEXT;
    v_event_payload JSONB;
    v_data JSONB;
    v_source_id TEXT;
BEGIN
    v_event_payload := NEW.payload::jsonb;

    -- Map Core event type → PRD webhook event type
    v_webhook_event_type := CASE NEW.event_type
        WHEN 'bank_account.opened'       THEN 'account.opened'
        WHEN 'bank_account.kyc_approved' THEN 'account.approved'
        WHEN 'bank_account.frozen'       THEN 'account.frozen'
        WHEN 'bank_account.closed'       THEN 'account.closed'
        ELSE NULL
    END;

    -- Skip unmapped events (deposited, withdrew, unfrozen)
    IF v_webhook_event_type IS NULL THEN
        RETURN NEW;
    END IF;

    -- Extract tenant_id from nested base_event
    SELECT (value -> 'base_event' ->> 'tenant_id')::INT
    INTO v_tenant_id
    FROM jsonb_each(v_event_payload)
    WHERE value -> 'base_event' ->> 'tenant_id' IS NOT NULL
    LIMIT 1;

    IF v_tenant_id IS NULL THEN
        RETURN NEW;
    END IF;

    -- Build source_id for dedup: "{aggregate_type}_{aggregate_id}_{sequence}"
    v_source_id := format('ba_%s_%s', NEW.aggregate_id, NEW.sequence);

    -- Build clean payload for tenant consumption
    v_data := jsonb_build_object(
        'account_id', NEW.aggregate_id,
        'event_type', v_webhook_event_type
    );

    INSERT INTO portal.webhook_events (
        tenant_id, event_type, aggregate_type, aggregate_id,
        source_id, payload
    ) VALUES (
        v_tenant_id, v_webhook_event_type, NEW.aggregate_type,
        NEW.aggregate_id, v_source_id, v_data
    );

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_capture_bank_account_webhook
    AFTER INSERT ON bank_account_events
    FOR EACH ROW
    EXECUTE FUNCTION portal.capture_bank_account_event();


-- Trigger 2: Capture transaction.completed from outbox processing
CREATE OR REPLACE FUNCTION portal.capture_transaction_completed()
RETURNS TRIGGER AS $$
BEGIN
    -- Only fire when processed changes from false to true
    IF OLD.processed = false AND NEW.processed = true THEN
        INSERT INTO portal.webhook_events (
            tenant_id, event_type, aggregate_type, aggregate_id,
            source_id, payload
        ) VALUES (
            NEW.tenant_id,
            'transaction.completed',
            'transaction',
            NEW.transaction_id::text,
            format('tx_%s', NEW.transaction_id),
            jsonb_build_object(
                'transaction_id', NEW.transaction_id,
                'event_type', NEW.event_type,
                'status', 'completed'
            )
        );
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_capture_transaction_completed
    AFTER UPDATE ON outbox
    FOR EACH ROW
    WHEN (OLD.processed = false AND NEW.processed = true)
    EXECUTE FUNCTION portal.capture_transaction_completed();


-- Trigger 3: Capture transaction.failed from dead letter
CREATE OR REPLACE FUNCTION portal.capture_transaction_failed()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO portal.webhook_events (
        tenant_id, event_type, aggregate_type, aggregate_id,
        source_id, payload
    ) VALUES (
        -- tenant_id not directly on outbox_dead_letter; derive from transaction_id
        -- or add tenant_id column to outbox_dead_letter in future
        COALESCE(
            (SELECT tenant_id FROM outbox WHERE transaction_id = NEW.transaction_id LIMIT 1),
            0
        ),
        'transaction.failed',
        'transaction',
        NEW.transaction_id::text,
        format('dl_%s', NEW.original_outbox_id),
        jsonb_build_object(
            'transaction_id', NEW.transaction_id,
            'error', NEW.error_message,
            'retry_count', NEW.retry_count,
            'status', 'failed'
        )
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_capture_transaction_failed
    AFTER INSERT ON outbox_dead_letter
    FOR EACH ROW
    EXECUTE FUNCTION portal.capture_transaction_failed();
```

### 12.3 Migration 3: Schema Updates to Existing Tables

```sql
-- 20260310000003_webhook_schema_updates.sql

-- Add event_source_id to webhook_deliveries for dedup
ALTER TABLE portal.webhook_deliveries
    ADD COLUMN IF NOT EXISTS event_source_id TEXT;

-- Unique constraint for dedup (prevent duplicate fan-out)
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_deliveries_dedup
    ON portal.webhook_deliveries (endpoint_id, event_source_id)
    WHERE event_source_id IS NOT NULL;

-- Add description to webhook_endpoints
ALTER TABLE portal.webhook_endpoints
    ADD COLUMN IF NOT EXISTS description TEXT;

-- Add signing secret rotation support
ALTER TABLE portal.webhook_endpoints
    ADD COLUMN IF NOT EXISTS old_signing_secret VARCHAR,
    ADD COLUMN IF NOT EXISTS secret_grace_expires_at TIMESTAMPTZ;

-- Optimize delivery polling (hot path)
CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_pending_retry
    ON portal.webhook_deliveries (next_retry_at)
    WHERE status = 'pending';

-- Optimize fan-out endpoint lookup
CREATE INDEX IF NOT EXISTS idx_webhook_endpoints_org_active
    ON portal.webhook_endpoints (org_id)
    WHERE status = 'active';

-- Extend api_logs partitions (May-Jul 2026 per PRD)
CREATE TABLE IF NOT EXISTS portal.api_logs_2026_05
    PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');
CREATE TABLE IF NOT EXISTS portal.api_logs_2026_06
    PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');
CREATE TABLE IF NOT EXISTS portal.api_logs_2026_07
    PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-07-01') TO ('2026-08-01');
```

### 12.4 New Crate Dependencies

```toml
# crates/bankie-gateway/Cargo.toml
hmac = "0.12"
sha2 = "0.10"       # already in workspace
hex = "0.4"          # already in workspace
reqwest = { version = "0.12", features = ["json"] }
```

### 12.5 SQLx Cache Regeneration

```bash
DATABASE_URL="postgres://bankie_app:password@localhost:5432/bankie_main" cargo sqlx prepare
```

---

## 13. Redis Key Patterns

| Key | TTL | Purpose |
|-----|-----|---------|
| `gw:lock:webhook_fanout` | 30s | Distributed lock — fan-out job |
| `gw:lock:webhook_deliver` | 60s | Distributed lock — delivery job |
| `gw:lock:grace_expiry` | 120s | Existing — grace period expiry job |

---

## 14. Testing Strategy

### 14.1 Unit Tests (Target: +27 tests, per PRD milestone M4)

| Area | Tests |
|------|-------|
| `signing.rs` | `sign_payload` correctness, empty body, timestamp format, `generate_signing_secret` format |
| `models/webhook.rs` | Serde round-trip, `VALID_EVENT_TYPES` validation, `EndpointStatus`/`DeliveryStatus` |
| `routes/webhook.rs` | CRUD validation, RBAC (Owner/Admin only for mutate), org isolation, URL HTTPS enforcement |
| `dispatcher.rs` | Fan-out matching logic, multi-endpoint, no-match skip, disabled endpoint skip, dedup on source_id |
| `deliverer.rs` | Retry schedule (verify delays match PRD), dead letter at attempt 8, circuit breaker at failure 5 |
| `redaction.rs` | IP masking (v4/v6), sensitive key redaction, nested object traversal |

### 14.2 E2E Scenarios

| Scenario | Flow |
|----------|------|
| Happy path | Create endpoint → deposit via API key → verify delivery record → verify signature |
| Retry | Point endpoint at mock 503 → verify attempt increments, next_retry_at matches schedule |
| Circuit breaker | 5 consecutive failures → verify endpoint auto-disabled |
| Dead letter | Exhaust 7 retries → verify `status = 'dead_letter'` |
| Dedup | Same event triggers twice → verify single delivery (unique constraint) |
| Secret rotation | Rotate → verify dual signatures during grace → verify single after expiry |

---

## 15. Rollout Plan (Per PRD: 2 devs, 4 days)

### Day 1–2: Backend (Dev-1 + Dev-2 in parallel)

**Dev-1 (T1 → T2 → T4):**
1. DB migrations (3 files)
2. `models/webhook.rs` + `repo/webhook.rs` (trait + PG impl + mock)
3. `routes/webhook.rs` (endpoint CRUD)
4. `webhook/dispatcher.rs` (fan-out job)
5. `webhook/deliverer.rs` (delivery job + retry)

**Dev-2 (T3 → T6 → T5):**
1. `webhook/signing.rs` (HMAC-SHA256)
2. `redaction.rs` (PII redaction)
3. Update `api_logger` middleware to use redaction
4. `routes/log.rs` (API logs viewer endpoint)
5. Delivery logs endpoint

### Day 3: SPA (Dev-2) + Tests (Dev-1)

**Dev-2 (T7 → T8):**
1. Webhooks page (endpoint list, create modal, delivery history)
2. API Logs page (filterable table)

**Dev-1 (T9):**
1. Unit tests for all backend modules
2. Wire fan-out + delivery jobs in `main.rs`
3. Dashboard webhook stats extension

### Day 4: Integration + Review

- E2E test: end-to-end webhook delivery flow
- `cargo clippy --all-targets --no-default-features --tests --benches -- -D warnings`
- `cargo fmt -- --check`
- Code review
- PR submission

---

## 16. Build vs Buy: In-House (Confirmed)

Per PRD Section 5, **build in-house**. Key factors:

| Factor | Score |
|--------|-------|
| Tables already exist | `portal.webhook_endpoints` + `webhook_deliveries` |
| Pattern reuse | Identical to grace expiry job (tokio::spawn + interval + Redis lock) |
| Cost | $0 vs $200+/mo Svix SaaS |
| Scale headroom | ~50K deliveries/day capacity (sufficient for <1000 tenants) |
| Revisit trigger | >1000 tenants OR >100K deliveries/day → evaluate Svix self-hosted |

---

## 17. Open Questions

| # | Question | Owner | Recommendation |
|---|----------|-------|---------------|
| Q7 | Dispatcher event source strategy | Architect | **Resolved**: DB triggers → staging table (see Section 2) |
| Q8 | Endpoint auto-disable threshold configurable? | Product | Fixed at 5 for v1 (per PRD) |
| Q9 | Future event types (ledger.credited, member.invited)? | Product | Phase 4+ backlog |
| Q10 | Max webhook endpoints per org? | Architect | Recommend 10 to bound fan-out. Enforced in `create_endpoint` handler. |
| Q11 | `outbox_dead_letter` lacks `tenant_id` column — trigger uses correlated subquery | Architect | Works but suboptimal. Consider adding `tenant_id` to `outbox_dead_letter` in Phase 4. |

---

## Appendix A: Webhook Headers Reference

| Header | Example | Description |
|--------|---------|-------------|
| `Content-Type` | `application/json` | Always JSON |
| `User-Agent` | `Bankie-Webhooks/1.0` | Source identification |
| `X-Bankie-Signature` | `t=1709366400,v1=5d3a...` | Timestamp + HMAC-SHA256 |
| `X-Bankie-Event-Id` | `evt_550e8400...` | Idempotency key |
| `X-Bankie-Webhook-Id` | `dlv_d4e5f6...` | Delivery tracking ID |

## Appendix B: Competitive Alignment

| Feature | Stripe | Svix | **Bankie Phase 3** |
|---------|--------|------|--------------------|
| Signing scheme | `t=,v1=` | `svix-id` + `svix-timestamp` + `svix-signature` | `t=,v1=` (Stripe-compatible) |
| Retry attempts | 8 / 72h | Configurable | 7 / ~17h |
| Circuit breaker | Auto-disable | Auto-disable | Auto-disable (5 failures) |
| Secret rotation | Dashboard | API | API (24h grace) |
| Delivery logs | Dashboard + API | Dashboard + API | Dashboard + API |
| Dedup | Event ID | Message ID | `event_source_id` unique constraint |
