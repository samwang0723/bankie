-- =============================================================================
-- Interest Engine: Rate configs, tiers, daily accruals, and postings
-- =============================================================================

-- Interest rate configuration (platform-admin, per currency + account_kind)
CREATE TABLE IF NOT EXISTS interest_rate_configs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    currency        VARCHAR(10) NOT NULL,
    account_kind    VARCHAR(20) NOT NULL DEFAULT 'Interest',
    day_count       VARCHAR(20) NOT NULL DEFAULT 'Actual/365',
    posting_frequency VARCHAR(10) NOT NULL DEFAULT 'Monthly',
    posting_day     SMALLINT,
    effective_from  DATE NOT NULL,
    effective_to    DATE,
    is_active       BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_rate_config_active UNIQUE (currency, account_kind, effective_from),
    CONSTRAINT chk_posting_frequency CHECK (
        (posting_frequency = 'Daily' AND posting_day IS NULL) OR
        (posting_frequency = 'Weekly' AND posting_day BETWEEN 1 AND 7) OR
        (posting_frequency = 'Monthly' AND posting_day BETWEEN 1 AND 28)
    )
);

-- Tiered rates per config (blended calculation)
CREATE TABLE IF NOT EXISTS interest_rate_tiers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    rate_config_id  UUID NOT NULL REFERENCES interest_rate_configs(id) ON DELETE CASCADE,
    tier_order      SMALLINT NOT NULL,
    min_balance     NUMERIC NOT NULL DEFAULT 0,
    max_balance     NUMERIC,
    apr             NUMERIC(10, 8) NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_tier_order UNIQUE (rate_config_id, tier_order),
    CONSTRAINT chk_balance_range CHECK (max_balance IS NULL OR max_balance > min_balance),
    CONSTRAINT chk_apr_positive CHECK (apr >= 0)
);

CREATE INDEX IF NOT EXISTS idx_rate_tiers_config ON interest_rate_tiers(rate_config_id, tier_order);

-- Daily interest accrual records (one row per account per day, read-side)
CREATE TABLE IF NOT EXISTS interest_accruals (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       INTEGER NOT NULL,
    account_id      VARCHAR(64) NOT NULL,
    ledger_id       VARCHAR(64) NOT NULL,
    currency        VARCHAR(10) NOT NULL,
    accrual_date    DATE NOT NULL,
    balance_used    NUMERIC NOT NULL,
    daily_interest  NUMERIC(28, 18) NOT NULL,
    rate_config_id  UUID NOT NULL REFERENCES interest_rate_configs(id),
    tier_breakdown  JSONB NOT NULL DEFAULT '[]',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_accrual_account_date UNIQUE (account_id, accrual_date)
);

CREATE INDEX IF NOT EXISTS idx_accrual_tenant_date ON interest_accruals(tenant_id, accrual_date);
CREATE INDEX IF NOT EXISTS idx_accrual_account_date ON interest_accruals(account_id, accrual_date);

-- Interest postings (tracks capitalization events)
CREATE TABLE IF NOT EXISTS interest_postings (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id           INTEGER NOT NULL,
    account_id          VARCHAR(64) NOT NULL,
    currency            VARCHAR(10) NOT NULL,
    posting_date        DATE NOT NULL,
    period_start        DATE NOT NULL,
    period_end          DATE NOT NULL,
    accrued_total       NUMERIC(28, 18) NOT NULL,
    posted_amount       NUMERIC NOT NULL,
    transaction_id      UUID,
    status              VARCHAR(20) NOT NULL DEFAULT 'pending',
    error_message       TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_posting_account_period UNIQUE (account_id, period_start, period_end)
);

CREATE INDEX IF NOT EXISTS idx_posting_status ON interest_postings(status, posting_date);
CREATE INDEX IF NOT EXISTS idx_posting_account ON interest_postings(account_id, posting_date DESC);
