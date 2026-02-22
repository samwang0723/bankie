-- Phase 0: Foundation Hardening Migration
-- Assets table, precision expansion, tenant isolation, indexes, dead letter, account sequence

-- =============================================================================
-- 0.1 Multi-Asset Type System
-- =============================================================================

CREATE TABLE assets (
    code VARCHAR(10) PRIMARY KEY,
    asset_class VARCHAR(10) NOT NULL CHECK (asset_class IN ('Fiat', 'Crypto')),
    precision INTEGER NOT NULL CHECK (precision BETWEEN 0 AND 18),
    min_amount DECIMAL(38, 18) NOT NULL DEFAULT 0,
    display_name VARCHAR(100) NOT NULL,
    network VARCHAR(50),
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Seed initial assets
INSERT INTO assets (code, asset_class, precision, min_amount, display_name) VALUES
    ('USD', 'Fiat', 2, 0.01, 'US Dollar'),
    ('TWD', 'Fiat', 0, 1, 'Taiwan Dollar'),
    ('BTC', 'Crypto', 8, 0.00000001, 'Bitcoin'),
    ('ETH', 'Crypto', 18, 0.000000000000000001, 'Ethereum'),
    ('USDT', 'Crypto', 6, 0.01, 'Tether USD');

-- Expand precision for crypto amounts
ALTER TABLE transactions ALTER COLUMN amount TYPE DECIMAL(38, 18);
ALTER TABLE journal_lines ALTER COLUMN debit_amount TYPE DECIMAL(38, 18);
ALTER TABLE journal_lines ALTER COLUMN credit_amount TYPE DECIMAL(38, 18);

-- Expand currency column width for longer asset codes
ALTER TABLE transactions ALTER COLUMN currency TYPE VARCHAR(10);
ALTER TABLE journal_lines ALTER COLUMN currency TYPE VARCHAR(10);
ALTER TABLE house_accounts ALTER COLUMN currency TYPE VARCHAR(10);

-- =============================================================================
-- 0.2 Tenant Isolation
-- =============================================================================

ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS tenant_id INTEGER;
ALTER TABLE ledger_views ADD COLUMN IF NOT EXISTS tenant_id INTEGER;
ALTER TABLE transactions ADD COLUMN IF NOT EXISTS tenant_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE journal_entries ADD COLUMN IF NOT EXISTS tenant_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE house_accounts ADD COLUMN IF NOT EXISTS tenant_id INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN IF NOT EXISTS tenant_id INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_bav_tenant ON bank_account_views(tenant_id);
CREATE INDEX IF NOT EXISTS idx_lv_tenant ON ledger_views(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tx_tenant ON transactions(tenant_id);
CREATE INDEX IF NOT EXISTS idx_je_tenant ON journal_entries(tenant_id);
CREATE INDEX IF NOT EXISTS idx_ha_tenant ON house_accounts(tenant_id);

-- Extract hot fields from JSON payload into real columns for indexed lookups
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS user_id TEXT;
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS asset_code VARCHAR(10);
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS kind VARCHAR(20);

CREATE INDEX IF NOT EXISTS idx_bav_user ON bank_account_views(tenant_id, user_id);
CREATE INDEX IF NOT EXISTS idx_bav_asset ON bank_account_views(tenant_id, asset_code);

-- =============================================================================
-- 0.3 Race Condition Fixes — Account creation uniqueness
-- =============================================================================

-- Unique constraint prevents duplicate accounts (TOCTOU fix for account creation)
-- Note: These columns are populated by the View update() and may be NULL for old rows
CREATE UNIQUE INDEX IF NOT EXISTS idx_bav_unique_account
    ON bank_account_views(tenant_id, user_id, asset_code, kind)
    WHERE user_id IS NOT NULL AND asset_code IS NOT NULL AND kind IS NOT NULL;

-- =============================================================================
-- 0.5 Account Number Uniqueness — Sequence-based generation
-- =============================================================================

CREATE SEQUENCE IF NOT EXISTS account_number_seq START WITH 1000000000;

-- =============================================================================
-- 0.6d Outbox Throughput — Dead letter table
-- =============================================================================

CREATE TABLE outbox_dead_letter (
    id SERIAL PRIMARY KEY,
    original_outbox_id INTEGER NOT NULL,
    transaction_id UUID NOT NULL,
    event_type VARCHAR(255),
    payload JSONB,
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Add retry tracking to outbox
ALTER TABLE outbox ADD COLUMN IF NOT EXISTS retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN IF NOT EXISTS last_error TEXT;

-- =============================================================================
-- 0.6e Hot Path Index Additions
-- =============================================================================

-- Outbox polling (partial index for unprocessed only)
CREATE INDEX IF NOT EXISTS idx_outbox_unprocessed
    ON outbox(processed, created_at) WHERE processed = false;

-- Transaction list by account (paginated query)
CREATE INDEX IF NOT EXISTS idx_tx_account_date
    ON transactions(bank_account_id, created_at DESC);

-- =============================================================================
-- 0.4 Idempotency — tracking table (Redis primary, DB fallback for audit)
-- =============================================================================

CREATE TABLE idempotency_keys (
    key VARCHAR(255) PRIMARY KEY,
    tenant_id INTEGER NOT NULL,
    response_status INTEGER NOT NULL,
    response_body JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_idempotency_expires ON idempotency_keys(expires_at);

-- =============================================================================
-- Phase 1 prep: Balance Snapshots table
-- =============================================================================

CREATE TABLE balance_snapshots (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id INTEGER NOT NULL,
    account_id TEXT NOT NULL,
    ledger_id TEXT NOT NULL,
    asset_code VARCHAR(10) NOT NULL,
    available DECIMAL(38, 18) NOT NULL,
    pending DECIMAL(38, 18) NOT NULL,
    current_balance DECIMAL(38, 18) NOT NULL,
    snapshot_date DATE NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(tenant_id, account_id, snapshot_date)
);
