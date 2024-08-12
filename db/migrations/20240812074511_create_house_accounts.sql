CREATE TABLE house_accounts (
    id uuid PRIMARY KEY,
    account_number varchar(20) UNIQUE NOT NULL,
    account_name varchar(100) NOT NULL,
    account_type varchar(50) NOT NULL,
    ledger_id varchar(36) NOT NULL,
    currency char(3) NOT NULL,
    created_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP,
    status varchar(20) NOT NULL DEFAULT 'active'
);

CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
   NEW.updated_at = NOW();
   RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_house_accounts_updated_at
BEFORE UPDATE ON house_accounts
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();
