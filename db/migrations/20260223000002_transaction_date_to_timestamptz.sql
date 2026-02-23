-- Change transaction_date from DATE to TIMESTAMPTZ for precise timestamps
ALTER TABLE transactions
    ALTER COLUMN transaction_date TYPE TIMESTAMPTZ
    USING transaction_date::TIMESTAMPTZ;
