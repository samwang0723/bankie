-- Remaining Phases: Transaction filtering indexes + balance snapshot index
-- Phase 3: Transaction History Enhancement
-- Phase 5: Balance Snapshots (table already exists from phase0)

-- =============================================================================
-- Transaction Filtering Indexes
-- =============================================================================

-- Composite index for filtered transaction queries (status + date range)
CREATE INDEX IF NOT EXISTS idx_tx_account_status
    ON transactions(bank_account_id, status);

CREATE INDEX IF NOT EXISTS idx_tx_account_date_range
    ON transactions(bank_account_id, transaction_date DESC);

-- Transaction reference prefix index for type filtering
CREATE INDEX IF NOT EXISTS idx_tx_reference_prefix
    ON transactions(bank_account_id, transaction_reference varchar_pattern_ops);

-- =============================================================================
-- Balance Snapshot Indexes
-- =============================================================================

CREATE INDEX IF NOT EXISTS idx_balance_snap_account_date
    ON balance_snapshots(account_id, snapshot_date DESC);

CREATE INDEX IF NOT EXISTS idx_balance_snap_tenant_date
    ON balance_snapshots(tenant_id, snapshot_date DESC);
