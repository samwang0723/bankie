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
    balance_id uuid NOT NULL,
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
