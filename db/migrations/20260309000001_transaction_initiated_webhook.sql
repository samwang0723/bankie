-- Add transaction.initiated webhook event
--
-- Fires AFTER INSERT on the transactions table to capture the moment a
-- transaction is first created (status='processing'). This fills the gap
-- between transaction creation and transaction.completed/transaction.failed
-- events already captured by existing triggers on outbox/outbox_dead_letter.
--
-- Follows the same pattern as fn_capture_bank_account_event,
-- fn_capture_transaction_completed, and fn_capture_transaction_failed.

-- =============================================================================
-- 1. Trigger function: Capture transaction initiated events
-- =============================================================================

CREATE OR REPLACE FUNCTION portal.fn_capture_transaction_initiated()
RETURNS TRIGGER AS $$
DECLARE
    event_payload JSONB;
    event_source VARCHAR;
    tx_type VARCHAR;
BEGIN
    -- Skip if no tenant context
    IF NEW.tenant_id IS NULL OR NEW.tenant_id = 0 THEN
        RETURN NEW;
    END IF;

    -- Derive transaction type from reference prefix
    -- Actual prefixes: DE (deposit), WI (withdrawal), TR (transfer), IN (interest)
    tx_type := CASE
        WHEN NEW.transaction_reference LIKE 'DE%' THEN 'deposit'
        WHEN NEW.transaction_reference LIKE 'WI%' THEN 'withdrawal'
        WHEN NEW.transaction_reference LIKE 'TR%' THEN 'transfer'
        WHEN NEW.transaction_reference LIKE 'IN%' THEN 'interest'
        ELSE 'unknown'
    END;

    -- Build webhook payload
    event_payload := jsonb_build_object(
        'transaction_id', NEW.id::TEXT,
        'bank_account_id', NEW.bank_account_id::TEXT,
        'transaction_reference', NEW.transaction_reference,
        'amount', NEW.amount::TEXT,
        'currency', NEW.currency,
        'transaction_type', tx_type,
        'status', NEW.status,
        'event_type', 'transaction.initiated'
    );

    -- Unique source: transaction_{uuid}
    event_source := 'transaction_' || NEW.id::TEXT;

    INSERT INTO portal.webhook_events
        (tenant_id, event_type, aggregate_type, aggregate_id, source_id, payload)
    VALUES
        (NEW.tenant_id, 'transaction.initiated', 'transaction',
         NEW.bank_account_id::TEXT, event_source, event_payload);

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- =============================================================================
-- 2. Trigger: Fire after every INSERT on transactions
-- =============================================================================

CREATE TRIGGER trg_capture_transaction_initiated
    AFTER INSERT ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION portal.fn_capture_transaction_initiated();
