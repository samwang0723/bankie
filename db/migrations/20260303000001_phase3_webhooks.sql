-- Phase 3: Webhooks & Observability — Schema Changes
-- Adds webhook_events staging table, DB triggers for event capture,
-- schema alterations for webhook_endpoints and webhook_deliveries,
-- and api_logs partitions for May–Jul 2026.

-- =============================================================================
-- 1. portal.webhook_events — Staging table for DB-trigger-captured events
-- =============================================================================

CREATE SEQUENCE IF NOT EXISTS portal.webhook_events_id_seq;

CREATE TABLE portal.webhook_events (
    id          BIGINT PRIMARY KEY DEFAULT nextval('portal.webhook_events_id_seq'),
    tenant_id   INT NOT NULL,
    event_type  VARCHAR NOT NULL,
    aggregate_type VARCHAR NOT NULL,
    aggregate_id   VARCHAR NOT NULL,
    source_id      VARCHAR NOT NULL,
    payload     JSONB NOT NULL,
    processed   BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_webhook_events_unprocessed
    ON portal.webhook_events (processed, created_at)
    WHERE processed = false;

CREATE INDEX idx_webhook_events_tenant_id
    ON portal.webhook_events (tenant_id);

-- =============================================================================
-- 2. ALTER portal.webhook_endpoints — Add description column
-- =============================================================================

ALTER TABLE portal.webhook_endpoints
    ADD COLUMN IF NOT EXISTS description VARCHAR;

-- =============================================================================
-- 3. ALTER portal.webhook_deliveries — Add event_source_id + unique constraint
-- =============================================================================

ALTER TABLE portal.webhook_deliveries
    ADD COLUMN IF NOT EXISTS event_source_id VARCHAR NOT NULL DEFAULT '';

-- Unique constraint prevents duplicate fan-out for same event to same endpoint
CREATE UNIQUE INDEX IF NOT EXISTS idx_webhook_deliveries_dedup
    ON portal.webhook_deliveries (endpoint_id, event_source_id)
    WHERE event_source_id != '';

-- =============================================================================
-- 4. DB Triggers — Event capture into portal.webhook_events
-- =============================================================================

-- 4a. Trigger function: Capture bank account lifecycle events
-- Maps Core's internal event names to PRD-specified webhook event types.
-- Only captures the 4 account lifecycle events; skips deposited/withdrew/unfrozen.
CREATE OR REPLACE FUNCTION portal.fn_capture_bank_account_event()
RETURNS TRIGGER AS $$
DECLARE
    webhook_event_type VARCHAR;
    event_tenant_id INT;
    event_payload JSONB;
    event_source VARCHAR;
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

    -- Extract tenant_id from the event payload's base_event
    -- bank_account_events.payload is json type, base_event contains tenant_id
    event_tenant_id := COALESCE(
        (NEW.payload::jsonb -> 'base_event' ->> 'tenant_id')::INT,
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

CREATE TRIGGER trg_capture_bank_account_event
    AFTER INSERT ON bank_account_events
    FOR EACH ROW
    EXECUTE FUNCTION portal.fn_capture_bank_account_event();

-- 4b. Trigger function: Capture transaction completed events
-- Fires when outbox.processed flips from false to true.
CREATE OR REPLACE FUNCTION portal.fn_capture_transaction_completed()
RETURNS TRIGGER AS $$
DECLARE
    event_payload JSONB;
    event_source VARCHAR;
BEGIN
    -- Only capture when processed transitions to true
    IF NEW.processed = true AND (OLD.processed = false OR OLD.processed IS NULL) THEN
        event_payload := jsonb_build_object(
            'transaction_id', NEW.transaction_id::TEXT,
            'event_type_detail', NEW.event_type,
            'raw_payload', NEW.payload
        );

        -- Unique source identifier: outbox_{id}
        event_source := 'outbox_' || NEW.id::TEXT;

        INSERT INTO portal.webhook_events (tenant_id, event_type, aggregate_type, aggregate_id, source_id, payload)
        VALUES (NEW.tenant_id, 'transaction.completed', 'outbox', NEW.transaction_id::TEXT, event_source, event_payload);
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_capture_transaction_completed
    AFTER UPDATE ON outbox
    FOR EACH ROW
    WHEN (NEW.processed = true)
    EXECUTE FUNCTION portal.fn_capture_transaction_completed();

-- 4c. Trigger function: Capture transaction failed events
-- Fires when a record is inserted into outbox_dead_letter (max retries exceeded).
CREATE OR REPLACE FUNCTION portal.fn_capture_transaction_failed()
RETURNS TRIGGER AS $$
DECLARE
    event_tenant_id INT;
    event_payload JSONB;
    event_source VARCHAR;
BEGIN
    -- Resolve tenant_id from the original outbox entry
    SELECT tenant_id INTO event_tenant_id
    FROM outbox
    WHERE id = NEW.original_outbox_id;

    -- If original outbox not found, skip
    IF event_tenant_id IS NULL THEN
        RETURN NEW;
    END IF;

    event_payload := jsonb_build_object(
        'transaction_id', NEW.transaction_id::TEXT,
        'original_outbox_id', NEW.original_outbox_id,
        'error_message', NEW.error_message,
        'retry_count', NEW.retry_count,
        'raw_payload', NEW.payload
    );

    -- Unique source identifier: dead_letter_{id}
    event_source := 'dead_letter_' || NEW.id::TEXT;

    INSERT INTO portal.webhook_events (tenant_id, event_type, aggregate_type, aggregate_id, source_id, payload)
    VALUES (event_tenant_id, 'transaction.failed', 'outbox_dead_letter', NEW.transaction_id::TEXT, event_source, event_payload);

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_capture_transaction_failed
    AFTER INSERT ON outbox_dead_letter
    FOR EACH ROW
    EXECUTE FUNCTION portal.fn_capture_transaction_failed();

-- =============================================================================
-- 5. api_logs partitions — May through July 2026
-- =============================================================================

CREATE TABLE portal.api_logs_2026_05 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-05-01') TO ('2026-06-01');

CREATE TABLE portal.api_logs_2026_06 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-06-01') TO ('2026-07-01');

CREATE TABLE portal.api_logs_2026_07 PARTITION OF portal.api_logs
    FOR VALUES FROM ('2026-07-01') TO ('2026-08-01');
