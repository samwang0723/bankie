CREATE TABLE journal_entries (
    id uuid PRIMARY KEY,
    entry_date date NOT NULL,
    description text,
    status varchar(20) NOT NULL,
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE journal_lines (
    id uuid PRIMARY KEY,
    journal_entry_id uuid NOT NULL REFERENCES journal_entries(id),
    balance_id uuid NOT NULL REFERENCES ledger_accounts(id),
    debit_amount decimal(19,4) NOT NULL DEFAULT 0,
    credit_amount decimal(19,4) NOT NULL DEFAULT 0,
    currency char(3) NOT NULL,
    description text,
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE transactions (
    id uuid PRIMARY KEY,
    bank_account_id uuid NOT NULL,
    transaction_reference varchar(64) NOT NULL UNIQUE,
    transaction_date date NOT NULL,
    amount decimal(19,4) NOT NULL,
    currency char(3) NOT NULL,
    description text,
    status varchar(20) NOT NULL,
    journal_entry_id uuid REFERENCES journal_entries(id),
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_journal_lines_journal_entry_id ON journal_lines(journal_entry_id);
CREATE INDEX idx_journal_lines_balance_id ON journal_lines(balance_id);
CREATE INDEX idx_transactions_bank_account_id ON transactions(bank_account_id);
CREATE INDEX idx_transactions_transaction_date ON transactions(transaction_date);
CREATE INDEX idx_transactions_journal_entry_id ON transactions(journal_entry_id);

-- -- Insert into transactions
-- INSERT INTO transactions (id, bank_account_id, transaction_reference, transaction_date, amount, currency, description, status)
-- VALUES (
--     gen_random_uuid(),
--     'customer_account_uuid_here',
--     'DEP12345',
--     CURRENT_DATE,
--     1000.00,
--     'USD',
--     'Deposit from external bank - John Doe',
--     'COMPLETED'
-- );
--
-- -- Insert into journal_entries
-- INSERT INTO journal_entries (id, entry_date, description, status)
-- VALUES (
--     gen_random_uuid(),
--     CURRENT_DATE,
--     'Deposit from external bank - John Doe',
--     'POSTED'
-- );
--
-- -- Insert into journal_lines (two entries for double-entry accounting)
-- INSERT INTO journal_lines (id, journal_entry_id, balance_id, debit_amount, credit_amount, currency, description)
-- VALUES
--     (gen_random_uuid(),
--      (SELECT id FROM journal_entries WHERE description = 'Deposit from external bank - John Doe' ORDER BY created_at DESC LIMIT 1),
--      (SELECT id FROM ledger_accounts WHERE account_name = 'Customer Deposits'),
--      1000.00, 0, 'USD', 'Deposit from external bank'),
--     (gen_random_uuid(),
--      (SELECT id FROM journal_entries WHERE description = 'Deposit from external bank - John Doe' ORDER BY created_at DESC LIMIT 1),
--      (SELECT id FROM ledger_accounts WHERE account_name = 'Customer Account Liabilities'),
--      0, 1000.00, 'USD', 'Deposit from external bank');
--
-- -- Update the transactions to link with the journal entry
-- UPDATE transactions
-- SET journal_entry_id = (SELECT id FROM journal_entries WHERE description = 'Deposit from external bank - John Doe' ORDER BY created_at DESC LIMIT 1)
-- WHERE transaction_reference = 'DEP12345';
--
