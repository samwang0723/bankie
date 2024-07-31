CREATE TABLE transactions (
    id uuid NOT NULL PRIMARY KEY,
    bank_account_id uuid NOT NULL,
    kind varchar(32) NOT NULL,
    credit_amount decimal NOT NULL DEFAULT 0.0,
    debit_amount decimal NOT NULL DEFAULT 0.0,
    status varchar(32) NOT NULL,
    meta json NOT NULL DEFAULT '{}',
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP
);
