-- Fix: add tenant_id to house account unique constraint for multi-tenant support
ALTER TABLE house_accounts DROP CONSTRAINT IF EXISTS unique_account_type_currency_status;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'unique_account_type_currency_status_tenant'
    ) THEN
        ALTER TABLE house_accounts ADD CONSTRAINT unique_account_type_currency_status_tenant
            UNIQUE (account_type, currency, status, tenant_id);
    END IF;
END $$;
