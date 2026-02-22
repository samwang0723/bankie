-- Account Model Refactor: optional external_reference_id, account_number, parent_id

-- Add new columns to bank_account_views for indexed lookups
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS account_number VARCHAR(20);
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS parent_id TEXT;
ALTER TABLE bank_account_views ADD COLUMN IF NOT EXISTS external_reference_id TEXT;

-- Unique constraint on account_number
CREATE UNIQUE INDEX IF NOT EXISTS idx_bav_account_number
    ON bank_account_views(account_number) WHERE account_number IS NOT NULL;

-- Index for parent_id lookups (sub-account query)
CREATE INDEX IF NOT EXISTS idx_bav_parent_id
    ON bank_account_views(parent_id) WHERE parent_id IS NOT NULL;

-- Index for external_reference_id lookups
CREATE INDEX IF NOT EXISTS idx_bav_external_ref
    ON bank_account_views(external_reference_id) WHERE external_reference_id IS NOT NULL;

-- Backfill external_reference_id from JSON payload user_id
UPDATE bank_account_views
SET external_reference_id = payload->>'user_id'
WHERE external_reference_id IS NULL AND payload->>'user_id' IS NOT NULL;

-- Backfill parent_id from JSON payload
UPDATE bank_account_views
SET parent_id = payload->>'parent_id'
WHERE parent_id IS NULL AND payload->>'parent_id' IS NOT NULL AND payload->>'parent_id' != '';

-- Update uniqueness constraint: use external_reference_id
DROP INDEX IF EXISTS idx_bav_unique_account;
CREATE UNIQUE INDEX IF NOT EXISTS idx_bav_unique_account
    ON bank_account_views(tenant_id, external_reference_id, asset_code, kind)
    WHERE external_reference_id IS NOT NULL AND asset_code IS NOT NULL AND kind IS NOT NULL;
