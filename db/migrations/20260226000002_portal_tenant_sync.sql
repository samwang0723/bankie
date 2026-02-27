-- Sync portal organizations to bankie-core tenants table
-- When a new portal org is created, auto-create a corresponding tenant entry

-- Function to sync portal org to tenants table
CREATE OR REPLACE FUNCTION portal.sync_org_to_tenant()
RETURNS TRIGGER AS $$
BEGIN
    INSERT INTO public.tenants (id, name, jwt, status, scope)
    VALUES (
        NEW.tenant_id,
        NEW.name,
        '', -- JWT not needed for portal-proxied requests
        'active',
        'accounts:read accounts:write ledgers:read ledgers:write transactions:read reports:read house_accounts:read house_accounts:write'
    )
    ON CONFLICT (id) DO UPDATE SET
        name = EXCLUDED.name,
        status = EXCLUDED.status;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger on portal.organizations insert
CREATE TRIGGER trg_sync_org_to_tenant
    AFTER INSERT ON portal.organizations
    FOR EACH ROW
    EXECUTE FUNCTION portal.sync_org_to_tenant();

-- Ensure tenants.id sequence is ahead of portal sequence to avoid conflicts
SELECT setval('tenants_id_seq', GREATEST(
    (SELECT last_value FROM tenants_id_seq),
    (SELECT last_value FROM portal.tenant_id_seq)
));
