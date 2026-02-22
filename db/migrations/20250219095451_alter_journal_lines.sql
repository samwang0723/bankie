-- Additional indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_transactions_status ON transactions(status);
CREATE INDEX IF NOT EXISTS idx_transactions_currency ON transactions(currency);
CREATE INDEX IF NOT EXISTS idx_outbox_processed ON outbox(processed);
CREATE INDEX IF NOT EXISTS idx_outbox_transaction_id ON outbox(transaction_id);
CREATE INDEX IF NOT EXISTS idx_house_accounts_currency ON house_accounts(currency);
CREATE INDEX IF NOT EXISTS idx_house_accounts_status ON house_accounts(status);
CREATE INDEX IF NOT EXISTS idx_journal_entries_status ON journal_entries(status);
