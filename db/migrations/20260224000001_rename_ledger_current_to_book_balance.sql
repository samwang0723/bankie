-- Rename the "current" key to "book_balance" in ledger_views JSON payload
-- to align with formal banking terminology (ISO 20022 CLBD = Closing Booked Balance)
-- Note: payload is json (not jsonb), so we cast for manipulation then back to json
UPDATE ledger_views
SET payload = (
    (payload::jsonb - 'current') || jsonb_build_object('book_balance', payload::jsonb->'current')
)::json
WHERE payload::jsonb ? 'current';
