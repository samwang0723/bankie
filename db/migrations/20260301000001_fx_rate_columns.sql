-- Add FX rate columns to transactions table
ALTER TABLE transactions
    ADD COLUMN fx_rate_to_usd DECIMAL(24,12),    -- Rate: 1 {currency} = X USD
    ADD COLUMN amount_usd     DECIMAL(19,2),     -- amount * fx_rate_to_usd, normalized to 2dp
    ADD COLUMN fx_rate_source  VARCHAR(50);       -- "coingecko", "exchangerate-api", "static"

-- Index for USD-based reporting and aggregation
CREATE INDEX idx_transactions_amount_usd ON transactions(amount_usd);

-- Comments for clarity
COMMENT ON COLUMN transactions.fx_rate_to_usd IS 'Exchange rate at transaction time: 1 unit of currency = X USD';
COMMENT ON COLUMN transactions.amount_usd IS 'Transaction amount converted to USD at creation-time rate';
COMMENT ON COLUMN transactions.fx_rate_source IS 'Source of the FX rate: coingecko, exchangerate-api, static, backfill';
