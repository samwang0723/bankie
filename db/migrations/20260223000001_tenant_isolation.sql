-- Tenant Isolation: triggers + missing columns
-- Ensures all data records know which tenant created/modified them.

-- =============================================================================
-- Add tenant_id to journal_lines (missing from phase0 migration)
-- =============================================================================
ALTER TABLE journal_lines ADD COLUMN IF NOT EXISTS tenant_id INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_jl_tenant ON journal_lines(tenant_id);

-- =============================================================================
-- Trigger: sync tenant_id + hot fields from JSON payload to indexed columns
-- on bank_account_views (managed by cqrs-es framework)
-- =============================================================================
CREATE OR REPLACE FUNCTION sync_bav_tenant_id() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.payload IS NOT NULL AND NEW.payload->>'tenant_id' IS NOT NULL THEN
        NEW.tenant_id = (NEW.payload->>'tenant_id')::INTEGER;
    END IF;
    IF NEW.payload IS NOT NULL AND NEW.payload->>'external_reference_id' IS NOT NULL THEN
        NEW.user_id = NEW.payload->>'external_reference_id';
    END IF;
    IF NEW.payload IS NOT NULL AND NEW.payload->>'currency' IS NOT NULL THEN
        NEW.asset_code = NEW.payload->>'currency';
    END IF;
    IF NEW.payload IS NOT NULL AND NEW.payload->>'kind' IS NOT NULL THEN
        NEW.kind = NEW.payload->>'kind';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_sync_bav_tenant ON bank_account_views;
CREATE TRIGGER trg_sync_bav_tenant
    BEFORE INSERT OR UPDATE ON bank_account_views
    FOR EACH ROW EXECUTE FUNCTION sync_bav_tenant_id();

-- =============================================================================
-- Trigger: sync tenant_id from JSON payload to indexed column
-- on ledger_views (managed by cqrs-es framework)
-- =============================================================================
CREATE OR REPLACE FUNCTION sync_lv_tenant_id() RETURNS TRIGGER AS $$
BEGIN
    IF NEW.payload IS NOT NULL AND NEW.payload->>'tenant_id' IS NOT NULL THEN
        NEW.tenant_id = (NEW.payload->>'tenant_id')::INTEGER;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_sync_lv_tenant ON ledger_views;
CREATE TRIGGER trg_sync_lv_tenant
    BEFORE INSERT OR UPDATE ON ledger_views
    FOR EACH ROW EXECUTE FUNCTION sync_lv_tenant_id();
