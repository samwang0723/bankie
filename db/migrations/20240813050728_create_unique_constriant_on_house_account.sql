ALTER TABLE house_accounts
ADD CONSTRAINT unique_account_type_currency_status UNIQUE (account_type, currency, status);
