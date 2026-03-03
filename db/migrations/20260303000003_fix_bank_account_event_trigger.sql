-- Fix: fn_capture_bank_account_event trigger extracts tenant_id incorrectly.
--
-- cqrs-es stores BankAccountEvent as a serde externally-tagged enum:
--   {"AccountOpened": {"base_event": {"tenant_id": N}, ...}}
--
-- The original trigger read: payload -> 'base_event' -> 'tenant_id'
-- which returns NULL because 'base_event' is nested inside the variant key.
--
-- Fix: unwrap the variant wrapper using jsonb_each() first.

CREATE OR REPLACE FUNCTION portal.fn_capture_bank_account_event()
RETURNS TRIGGER AS $$
DECLARE
    webhook_event_type VARCHAR;
    event_tenant_id INT;
    event_payload JSONB;
    event_source VARCHAR;
    inner_payload JSONB;
BEGIN
    -- Map Core event types to webhook event types
    webhook_event_type := CASE NEW.event_type
        WHEN 'bank_account.opened'       THEN 'account.opened'
        WHEN 'bank_account.kyc_approved' THEN 'account.approved'
        WHEN 'bank_account.frozen'       THEN 'account.frozen'
        WHEN 'bank_account.closed'       THEN 'account.closed'
        ELSE NULL
    END;

    -- Skip unmapped events (deposited, withdrew, unfrozen)
    IF webhook_event_type IS NULL THEN
        RETURN NEW;
    END IF;

    -- Extract inner payload from serde externally-tagged enum wrapper
    -- {"AccountOpened": {"base_event": {"tenant_id": N}, ...}} → inner = {"base_event": ...}
    SELECT value INTO inner_payload FROM jsonb_each(NEW.payload::jsonb) LIMIT 1;

    -- Extract tenant_id from the unwrapped inner payload
    event_tenant_id := COALESCE(
        (inner_payload -> 'base_event' ->> 'tenant_id')::INT,
        0
    );

    -- Skip events with no tenant context
    IF event_tenant_id = 0 THEN
        RETURN NEW;
    END IF;

    -- Build a clean payload for webhook consumption
    event_payload := jsonb_build_object(
        'account_id', NEW.aggregate_id,
        'event_type', webhook_event_type,
        'raw_payload', NEW.payload::jsonb
    );

    -- Unique source identifier for dedup: {aggregate_type}_{aggregate_id}_{sequence}
    event_source := NEW.aggregate_type || '_' || NEW.aggregate_id || '_' || NEW.sequence::TEXT;

    INSERT INTO portal.webhook_events (tenant_id, event_type, aggregate_type, aggregate_id, source_id, payload)
    VALUES (event_tenant_id, webhook_event_type, NEW.aggregate_type, NEW.aggregate_id, event_source, event_payload);

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
