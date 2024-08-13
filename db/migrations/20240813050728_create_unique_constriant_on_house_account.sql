ALTER TABLE house_accounts
ADD CONSTRAINT unique_currency_status UNIQUE (currency, status);
