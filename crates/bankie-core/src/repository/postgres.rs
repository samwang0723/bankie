use crate::common::asset::{Asset, AssetClass};
use crate::common::money::{Currency, Money};
use crate::domain::finance::{
    BalanceSnapshot, JournalEntry, JournalLine, Outbox, SettlementReportRow, Transaction,
    TRANS_DEPOSIT, TRANS_TRANSFER, TRANS_WITHDRAWAL,
};
use crate::domain::models::{BankAccountKind, HouseAccount, LedgerAction};
use crate::domain::tenant::Tenant;
use crate::domain::user::BankAccountWithLedger;
use crate::event_sourcing::command::LedgerCommand;

use super::adapter::DatabaseClient;
use async_trait::async_trait;
use chrono::{Local, NaiveDate};
use rust_decimal::Decimal;
use serde_json::to_value;
use sqlx::postgres::PgPool;
use sqlx::Error;
use uuid::Uuid;

#[async_trait]
impl DatabaseClient for PgPool {
    async fn get_user_bank_accounts(
        &self,
        user_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        let accounts = sqlx::query_as::<_, BankAccountWithLedger>(
            r#"
                select
                    b.payload->>'id' as id,
                    b.payload->>'account_number' as account_number,
                    b.payload->>'parent_id' as parent_id,
                    b.payload->>'status' as status,
                    b.payload->>'account_type' as account_type,
                    b.payload->>'kind' as kind,
                    b.payload->>'currency' as currency,
                    b.payload->>'ledger_id' as ledger_id,
                    (l.payload->'available'->>'amount')::numeric as available,
                    (l.payload->'pending'->>'amount')::numeric as pending,
                    (l.payload->'book_balance'->>'amount')::numeric as book_balance,
                    b.payload->>'created_at' as created_at,
                    b.payload->>'updated_at' as updated_at,
                    b.tenant_id
                from bank_account_views b
                left join ledger_views l on b.payload->>'ledger_id' = l.view_id
                where (b.payload->>'user_id' = $1
                   or b.payload->>'external_reference_id' = $1
                   or b.external_reference_id = $1)
                and b.tenant_id = $2
            "#,
        )
        .bind(&user_id)
        .bind(tenant_id)
        .fetch_all(self)
        .await?;

        Ok(accounts)
    }

    async fn fail_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        let mut tx = self.begin().await?;

        sqlx::query!(
            r#"
            UPDATE transactions
            SET status = 'failed', updated_at = NOW()
            WHERE id = $1
            "#,
            transaction_id,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            DELETE FROM outbox
            WHERE transaction_id = $1
            "#,
            transaction_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        let mut tx = self.begin().await?;

        sqlx::query!(
            r#"
            UPDATE transactions
            SET status = 'completed', updated_at = NOW()
            WHERE id = $1
            "#,
            transaction_id,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE outbox
            SET processed = true, processed_at = NOW()
            WHERE transaction_id = $1
            "#,
            transaction_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
        tenant_id: i32,
    ) -> Result<Uuid, Error> {
        let mut tx = self.begin().await?;

        // Insert JournalEntry
        let journal_entry_id = sqlx::query!(
            r#"
            INSERT INTO journal_entries (id, entry_date, description, status, tenant_id)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id
            "#,
            journal_entry.id,
            journal_entry.entry_date,
            journal_entry.description,
            journal_entry.status,
            tenant_id
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        // Insert JournalLines
        for journal_line in journal_lines {
            sqlx::query!(
                r#"
                INSERT INTO journal_lines (id, journal_entry_id, ledger_id, debit_amount, credit_amount, currency, description, tenant_id)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
                journal_line.id,
                journal_entry_id,
                journal_line.ledger_id,
                journal_line.debit_amount,
                journal_line.credit_amount,
                journal_line.currency,
                journal_line.description,
                tenant_id
            )
            .execute(&mut *tx)
            .await?;
        }

        // Insert Transaction
        let transaction_id = sqlx::query!(
            r#"
            INSERT INTO transactions (id, bank_account_id, transaction_reference,
            transaction_date, amount, currency, description, metadata, status, journal_entry_id, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id
            "#,
            transaction.id,
            transaction.bank_account_id,
            transaction.transaction_reference,
            transaction.transaction_date,
            transaction.amount,
            transaction.currency,
            transaction.description,
            transaction.metadata,
            transaction.status,
            journal_entry_id,
            tenant_id
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        // Insert Outbox
        let transaction_type = transaction.transaction_type();
        let event_type = if transaction_type == LedgerAction::Deposit {
            "LedgerCommand::Credit"
        } else {
            "LedgerCommand::Debit"
        };
        let cmd = if transaction_type == LedgerAction::Deposit {
            LedgerCommand::Credit {
                id: Uuid::parse_str(&ledger_id).map_err(|e| Error::Protocol(e.to_string()))?,
                account_id: transaction.bank_account_id,
                transaction_id,
                amount: Money::new(transaction.amount, Currency::from(transaction.currency)),
                tenant_id,
            }
        } else {
            LedgerCommand::DebitRelease {
                id: Uuid::parse_str(&ledger_id).map_err(|e| Error::Protocol(e.to_string()))?,
                account_id: transaction.bank_account_id,
                transaction_id,
                amount: Money::new(transaction.amount, Currency::from(transaction.currency)),
                tenant_id,
            }
        };
        let payload = to_value(&cmd).map_err(|e| Error::Protocol(e.to_string()))?;
        sqlx::query!(
            r#"
            INSERT INTO outbox (transaction_id, event_type, payload, tenant_id)
            VALUES ($1, $2, $3, $4)
            "#,
            transaction_id,
            event_type,
            payload,
            tenant_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(transaction_id)
    }

    async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO house_accounts (id, account_number, account_name, account_type, ledger_id, currency, status, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
            account.id,
            account.account_number,
            account.account_name,
            account.account_type,
            account.ledger_id,
            account.currency,
            account.status,
            account.tenant_id
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn get_house_account(
        &self,
        asset_code: &str,
        tenant_id: i32,
    ) -> Result<HouseAccount, Error> {
        let house_account = sqlx::query_as!(
            HouseAccount,
            r#"
            SELECT id, status, account_number, account_name, account_type, ledger_id, currency, tenant_id
            FROM house_accounts
            WHERE currency = $1
            AND tenant_id = $2
            AND status = 'active'
            AND account_type = 'House'
            LIMIT 1
            "#,
            asset_code,
            tenant_id
        )
        .fetch_one(self)
        .await?;

        Ok(house_account)
    }

    async fn get_house_accounts(
        &self,
        asset_code: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<HouseAccount>, Error> {
        let house_accounts = sqlx::query_as!(
            HouseAccount,
            r#"
            SELECT id, status, account_number, account_name, account_type, ledger_id, currency, tenant_id
            FROM house_accounts
            WHERE ($1::text IS NULL OR currency = $1)
            AND tenant_id = $2
            AND status = 'active'
            "#,
            asset_code,
            tenant_id
        )
        .fetch_all(self)
        .await?;

        Ok(house_accounts)
    }

    async fn validate_bank_account_exists(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        kind: BankAccountKind,
        tenant_id: i32,
    ) -> Result<bool, Error> {
        let ext_ref = match external_reference_id {
            Some(ref r) if !r.is_empty() => r.clone(),
            _ => return Ok(true), // No external ref = no dedup needed
        };
        let currency_str = currency.to_string();
        let kind_str = kind.to_string();
        let row = sqlx::query_scalar::<_, i64>(
            r#"
            select count(1) from bank_account_views
            where (payload->>'user_id'=$1 OR payload->>'external_reference_id'=$1 OR external_reference_id=$1)
            and payload->>'currency'=$2
            and payload->>'kind'=$3
            and payload->>'status' IN ('Pending', 'Approved', 'Freeze')
            and tenant_id=$4
            "#,
        )
        .bind(&ext_ref)
        .bind(&currency_str)
        .bind(&kind_str)
        .bind(tenant_id)
        .fetch_one(self)
        .await?;

        Ok(row == 0)
    }

    async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        tenant_id: i32,
    ) -> Result<Option<String>, Error> {
        let ext_ref = match external_reference_id {
            Some(ref r) if !r.is_empty() => r.clone(),
            _ => return Ok(None),
        };
        let result = sqlx::query_scalar::<_, String>(
            r#"
            SELECT view_id FROM bank_account_views
            WHERE (payload->>'user_id' = $1 OR payload->>'external_reference_id' = $1 OR external_reference_id = $1)
            AND payload->>'currency' = $2
            AND payload->>'kind' = 'Checking'
            AND payload->>'status' IN ('Pending', 'Approved')
            AND tenant_id = $3
            LIMIT 1
            "#,
        )
        .bind(&ext_ref)
        .bind(currency)
        .bind(tenant_id)
        .fetch_optional(self)
        .await?;

        Ok(result)
    }

    async fn get_sub_accounts(
        &self,
        account_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        let accounts = sqlx::query_as::<_, BankAccountWithLedger>(
            r#"
            SELECT
                b.payload->>'id' as id,
                b.payload->>'account_number' as account_number,
                b.payload->>'parent_id' as parent_id,
                b.payload->>'status' as status,
                b.payload->>'account_type' as account_type,
                b.payload->>'kind' as kind,
                b.payload->>'currency' as currency,
                b.payload->>'ledger_id' as ledger_id,
                (l.payload->'available'->>'amount')::numeric as available,
                (l.payload->'pending'->>'amount')::numeric as pending,
                (l.payload->'book_balance'->>'amount')::numeric as book_balance,
                b.payload->>'created_at' as created_at,
                b.payload->>'updated_at' as updated_at,
                b.tenant_id
            FROM bank_account_views b
            LEFT JOIN ledger_views l ON b.payload->>'ledger_id' = l.view_id
            WHERE (b.view_id = $1
               OR b.parent_id = $1
               OR b.payload->>'parent_id' = $1)
            AND b.tenant_id = $2
            "#,
        )
        .bind(&account_id)
        .bind(tenant_id)
        .fetch_all(self)
        .await?;

        Ok(accounts)
    }

    async fn get_bank_account_by_number(
        &self,
        account_number: String,
        tenant_id: i32,
    ) -> Result<BankAccountWithLedger, Error> {
        let account = sqlx::query_as::<_, BankAccountWithLedger>(
            r#"
            SELECT
                b.payload->>'id' as id,
                b.payload->>'account_number' as account_number,
                b.payload->>'parent_id' as parent_id,
                b.payload->>'status' as status,
                b.payload->>'account_type' as account_type,
                b.payload->>'kind' as kind,
                b.payload->>'currency' as currency,
                b.payload->>'ledger_id' as ledger_id,
                (l.payload->'available'->>'amount')::numeric as available,
                (l.payload->'pending'->>'amount')::numeric as pending,
                (l.payload->'book_balance'->>'amount')::numeric as book_balance,
                b.payload->>'created_at' as created_at,
                b.payload->>'updated_at' as updated_at,
                b.tenant_id
            FROM bank_account_views b
            LEFT JOIN ledger_views l ON b.payload->>'ledger_id' = l.view_id
            WHERE (b.account_number = $1
               OR b.payload->>'account_number' = $1)
            AND b.tenant_id = $2
            LIMIT 1
            "#,
        )
        .bind(&account_number)
        .bind(tenant_id)
        .fetch_one(self)
        .await?;

        Ok(account)
    }

    async fn create_tenant_profile(&self, name: &str, scope: &str) -> Result<i32, Error> {
        let rec = sqlx::query!(
            r#"
            INSERT INTO tenants (name, status, jwt, scope)
            VALUES ($1, 'inactive', '', $2)
            RETURNING id
            "#,
            name,
            scope
        )
        .fetch_one(self)
        .await?;

        Ok(rec.id)
    }

    async fn update_tenant_profile(&self, id: i32, jwt: &str) -> Result<i32, Error> {
        let dt = Local::now();
        let naive_utc = dt.naive_utc();
        let rec = sqlx::query!(
            r#"
            UPDATE tenants
            SET jwt = $2, status = 'active', updated_at = $3
            WHERE id = $1
            RETURNING id
            "#,
            id,
            jwt,
            naive_utc
        )
        .fetch_one(self)
        .await?;

        Ok(rec.id)
    }

    async fn get_tenant_profile(&self, tenant_id: i32) -> Result<Tenant, Error> {
        let rec = sqlx::query!(
            r#"
            SELECT id, name, jwt, status, scope
            FROM tenants
            WHERE id = $1 AND status='active'
            "#,
            tenant_id
        )
        .fetch_one(self)
        .await?;

        Ok(Tenant {
            id: rec.id,
            name: rec.name,
            jwt: rec.jwt,
            status: rec.status.expect("no status"),
            scope: Some(rec.scope.expect("no scope")),
        })
    }

    async fn get_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error> {
        let outbox = sqlx::query_as!(
            Outbox,
            r#"
            SELECT id, transaction_id, event_type, payload, processed, retry_count, tenant_id
            FROM outbox
            WHERE processed = false AND retry_count < 5
            ORDER BY created_at ASC
            LIMIT 1000
            "#,
        )
        .fetch_all(self)
        .await?;

        Ok(outbox)
    }

    async fn get_transactions(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error> {
        let account_id = bank_account_id
            .map(|id| Uuid::parse_str(&id).map_err(|e| Error::Protocol(e.to_string())))
            .transpose()?;
        let transactions = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT
                id,
                bank_account_id,
                transaction_reference,
                transaction_date,
                amount,
                currency,
                description,
                metadata,
                status,
                journal_entry_id,
                tenant_id
            FROM transactions
            WHERE ($1::uuid IS NULL OR bank_account_id = $1)
            AND tenant_id = $2
            ORDER BY created_at DESC
            OFFSET $3 LIMIT $4
            "#,
        )
        .bind(account_id)
        .bind(tenant_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(self)
        .await?;

        Ok(transactions)
    }

    async fn get_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error> {
        let account_id = bank_account_id
            .map(|id| Uuid::parse_str(&id).map_err(|e| Error::Protocol(e.to_string())))
            .transpose()?;

        let ref_prefix = transaction_type.as_deref().map(|t| match t {
            "deposit" => TRANS_DEPOSIT,
            "withdrawal" => TRANS_WITHDRAWAL,
            "transfer" => TRANS_TRANSFER,
            _ => "",
        });

        let transactions = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT
                id, bank_account_id, transaction_reference, transaction_date,
                amount, currency, description, metadata, status, journal_entry_id, tenant_id
            FROM transactions
            WHERE ($1::uuid IS NULL OR bank_account_id = $1)
              AND tenant_id = $8
              AND ($4::date IS NULL OR transaction_date >= $4::date::timestamptz)
              AND ($5::date IS NULL OR transaction_date < ($5::date + 1)::timestamptz)
              AND ($6::text IS NULL OR $6 = '' OR transaction_reference LIKE $6 || '%')
              AND ($7::text IS NULL OR status = $7)
            ORDER BY created_at DESC
            OFFSET $2 LIMIT $3
            "#,
        )
        .bind(account_id)
        .bind(offset)
        .bind(limit)
        .bind(start_date)
        .bind(end_date)
        .bind(ref_prefix)
        .bind(status.as_deref())
        .bind(tenant_id)
        .fetch_all(self)
        .await?;

        Ok(transactions)
    }

    async fn count_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<i64, Error> {
        let account_id = bank_account_id
            .map(|id| Uuid::parse_str(&id).map_err(|e| Error::Protocol(e.to_string())))
            .transpose()?;

        let ref_prefix = transaction_type.as_deref().map(|t| match t {
            "deposit" => TRANS_DEPOSIT,
            "withdrawal" => TRANS_WITHDRAWAL,
            "transfer" => TRANS_TRANSFER,
            _ => "",
        });

        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(1) FROM transactions
            WHERE ($1::uuid IS NULL OR bank_account_id = $1)
              AND tenant_id = $6
              AND ($2::date IS NULL OR transaction_date >= $2::date::timestamptz)
              AND ($3::date IS NULL OR transaction_date < ($3::date + 1)::timestamptz)
              AND ($4::text IS NULL OR $4 = '' OR transaction_reference LIKE $4 || '%')
              AND ($5::text IS NULL OR status = $5)
            "#,
        )
        .bind(account_id)
        .bind(start_date)
        .bind(end_date)
        .bind(ref_prefix)
        .bind(status.as_deref())
        .bind(tenant_id)
        .fetch_one(self)
        .await?;

        Ok(count)
    }

    async fn create_transfer_transactions(
        &self,
        source_account_id: Uuid,
        source_ledger_id: String,
        dest_account_id: Uuid,
        dest_ledger_id: String,
        amount: Money,
        tenant_id: i32,
    ) -> Result<Uuid, Error> {
        let mut tx = self.begin().await?;

        // Create journal entry for the transfer
        let journal_entry_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO journal_entries (id, entry_date, description, status, tenant_id)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(journal_entry_id)
        .bind(chrono::Utc::now().date_naive())
        .bind(format!("Transfer {} {}", amount.amount, amount.currency))
        .bind("posted")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        // Source journal line (debit)
        sqlx::query(
            r#"
            INSERT INTO journal_lines (id, journal_entry_id, ledger_id, debit_amount, credit_amount, currency, description, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(journal_entry_id)
        .bind(&source_ledger_id)
        .bind(amount.amount)
        .bind(rust_decimal::Decimal::ZERO)
        .bind(amount.currency.to_string())
        .bind("Transfer out")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        // Destination journal line (credit)
        sqlx::query(
            r#"
            INSERT INTO journal_lines (id, journal_entry_id, ledger_id, debit_amount, credit_amount, currency, description, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(journal_entry_id)
        .bind(&dest_ledger_id)
        .bind(rust_decimal::Decimal::ZERO)
        .bind(amount.amount)
        .bind(amount.currency.to_string())
        .bind("Transfer in")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        let currency_str = amount.currency.to_string();
        let now = chrono::Utc::now();

        // Source transaction (debit/withdrawal side)
        let source_tx_id = Uuid::new_v4();
        let source_ref = crate::common::snowflake::generate_transaction_reference(TRANS_TRANSFER);
        sqlx::query(
            r#"
            INSERT INTO transactions (id, bank_account_id, transaction_reference, transaction_date, amount, currency, description, metadata, status, journal_entry_id, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(source_tx_id)
        .bind(source_account_id)
        .bind(&source_ref)
        .bind(now)
        .bind(amount.amount)
        .bind(&currency_str)
        .bind("Transfer out")
        .bind(serde_json::Value::Null)
        .bind("processing")
        .bind(journal_entry_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        // Destination transaction (credit side)
        let dest_tx_id = Uuid::new_v4();
        let dest_ref = crate::common::snowflake::generate_transaction_reference(TRANS_TRANSFER);
        sqlx::query(
            r#"
            INSERT INTO transactions (id, bank_account_id, transaction_reference, transaction_date, amount, currency, description, metadata, status, journal_entry_id, tenant_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(dest_tx_id)
        .bind(dest_account_id)
        .bind(&dest_ref)
        .bind(now)
        .bind(amount.amount)
        .bind(&currency_str)
        .bind("Transfer in")
        .bind(serde_json::Value::Null)
        .bind("processing")
        .bind(journal_entry_id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        // Outbox: debit-release on source ledger
        let source_ledger_uuid =
            Uuid::parse_str(&source_ledger_id).map_err(|e| Error::Protocol(e.to_string()))?;
        let source_cmd = LedgerCommand::DebitRelease {
            id: source_ledger_uuid,
            account_id: source_account_id,
            transaction_id: source_tx_id,
            amount,
            tenant_id,
        };
        let source_payload = to_value(&source_cmd).map_err(|e| Error::Protocol(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO outbox (transaction_id, event_type, payload, tenant_id)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(source_tx_id)
        .bind("LedgerCommand::Debit")
        .bind(&source_payload)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        // Outbox: credit on destination ledger
        let dest_ledger_uuid =
            Uuid::parse_str(&dest_ledger_id).map_err(|e| Error::Protocol(e.to_string()))?;
        let dest_cmd = LedgerCommand::Credit {
            id: dest_ledger_uuid,
            account_id: dest_account_id,
            transaction_id: dest_tx_id,
            amount,
            tenant_id,
        };
        let dest_payload = to_value(&dest_cmd).map_err(|e| Error::Protocol(e.to_string()))?;
        sqlx::query(
            r#"
            INSERT INTO outbox (transaction_id, event_type, payload, tenant_id)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(dest_tx_id)
        .bind("LedgerCommand::Credit")
        .bind(&dest_payload)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // Return source transaction ID for debit-hold
        Ok(source_tx_id)
    }

    async fn move_to_dead_letter(
        &self,
        outbox_id: i32,
        transaction_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
        error_message: &str,
        retry_count: i32,
    ) -> Result<(), Error> {
        let mut tx = self.begin().await?;

        sqlx::query!(
            r#"
            INSERT INTO outbox_dead_letter (original_outbox_id, transaction_id, event_type, payload, error_message, retry_count)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            outbox_id,
            transaction_id,
            event_type,
            payload,
            error_message,
            retry_count,
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE outbox SET processed = true, processed_at = NOW()
            WHERE id = $1
            "#,
            outbox_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }

    async fn increment_outbox_retry(
        &self,
        outbox_id: i32,
        error_message: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE outbox
            SET retry_count = retry_count + 1, last_error = $2
            WHERE id = $1
            "#,
            outbox_id,
            error_message,
        )
        .execute(self)
        .await?;

        Ok(())
    }

    async fn load_assets(&self) -> Result<Vec<Asset>, Error> {
        let rows = sqlx::query!(
            r#"
            SELECT code, asset_class, precision, min_amount, display_name, network, is_active
            FROM assets
            WHERE is_active = true
            "#
        )
        .fetch_all(self)
        .await?;

        let assets = rows
            .into_iter()
            .map(|r| {
                let asset_class = match r.asset_class.as_str() {
                    "Crypto" => AssetClass::Crypto,
                    _ => AssetClass::Fiat,
                };
                Asset {
                    code: r.code,
                    asset_class,
                    precision: r.precision as u32,
                    min_amount: r.min_amount,
                    display_name: r.display_name,
                    network: r.network,
                    is_active: r.is_active,
                }
            })
            .collect();

        Ok(assets)
    }

    async fn create_balance_snapshot(&self, snapshot: BalanceSnapshot) -> Result<(), Error> {
        sqlx::query(
            r#"
            INSERT INTO balance_snapshots (id, tenant_id, account_id, ledger_id, asset_code, available, pending, current_balance, snapshot_date)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (tenant_id, account_id, snapshot_date) DO UPDATE
            SET available = $6, pending = $7, current_balance = $8
            "#,
        )
        .bind(snapshot.id)
        .bind(snapshot.tenant_id)
        .bind(&snapshot.account_id)
        .bind(&snapshot.ledger_id)
        .bind(&snapshot.asset_code)
        .bind(snapshot.available)
        .bind(snapshot.pending)
        .bind(snapshot.current_balance)
        .bind(snapshot.snapshot_date)
        .execute(self)
        .await?;

        Ok(())
    }

    async fn get_balance_history(
        &self,
        account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<BalanceSnapshot>, Error> {
        let snapshots = sqlx::query_as::<_, BalanceSnapshot>(
            r#"
            SELECT id, tenant_id, account_id, ledger_id, asset_code, available, pending, current_balance, snapshot_date
            FROM balance_snapshots
            WHERE account_id = $1
            AND tenant_id = $4
            AND snapshot_date >= $2
            AND snapshot_date <= $3
            ORDER BY snapshot_date ASC
            "#,
        )
        .bind(&account_id)
        .bind(start_date)
        .bind(end_date)
        .bind(tenant_id)
        .fetch_all(self)
        .await?;

        Ok(snapshots)
    }

    async fn get_all_active_account_balances(&self) -> Result<Vec<BankAccountWithLedger>, Error> {
        let accounts = sqlx::query_as::<_, BankAccountWithLedger>(
            r#"
            SELECT
                b.payload->>'id' as id,
                b.payload->>'account_number' as account_number,
                b.payload->>'parent_id' as parent_id,
                b.payload->>'status' as status,
                b.payload->>'account_type' as account_type,
                b.payload->>'kind' as kind,
                b.payload->>'currency' as currency,
                b.payload->>'ledger_id' as ledger_id,
                (l.payload->'available'->>'amount')::numeric as available,
                (l.payload->'pending'->>'amount')::numeric as pending,
                (l.payload->'book_balance'->>'amount')::numeric as book_balance,
                b.payload->>'created_at' as created_at,
                b.payload->>'updated_at' as updated_at,
                b.tenant_id
            FROM bank_account_views b
            LEFT JOIN ledger_views l ON b.payload->>'ledger_id' = l.view_id
            WHERE b.payload->>'status' = 'Approved'
            "#,
        )
        .fetch_all(self)
        .await?;

        Ok(accounts)
    }

    async fn get_settlement_report_data(
        &self,
        bank_account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<SettlementReportRow>, Error> {
        let account_id =
            Uuid::parse_str(&bank_account_id).map_err(|e| Error::Protocol(e.to_string()))?;

        let rows = sqlx::query_as::<_, SettlementReportRow>(
            r#"
            SELECT
                t.transaction_date,
                t.transaction_reference,
                t.amount,
                t.currency,
                t.description,
                t.status,
                t.journal_entry_id,
                COALESCE(jl.debit_amount, 0) as debit_amount,
                COALESCE(jl.credit_amount, 0) as credit_amount,
                b.payload->>'account_number' as account_number
            FROM transactions t
            LEFT JOIN journal_entries je ON t.journal_entry_id = je.id
            LEFT JOIN journal_lines jl ON je.id = jl.journal_entry_id
                AND jl.ledger_id = (
                    SELECT payload->>'ledger_id'
                    FROM bank_account_views
                    WHERE view_id = $1::text
                    AND tenant_id = $4
                )
            LEFT JOIN bank_account_views b ON b.view_id = $1::text AND b.tenant_id = $4
            WHERE t.bank_account_id = $1
                AND t.tenant_id = $4
                AND t.transaction_date >= $2::date::timestamptz
                AND t.transaction_date < ($3::date + 1)::timestamptz
                AND t.status IN ('completed', 'processing')
            ORDER BY t.transaction_date ASC, t.created_at ASC
            "#,
        )
        .bind(account_id)
        .bind(start_date)
        .bind(end_date)
        .bind(tenant_id)
        .fetch_all(self)
        .await?;

        Ok(rows)
    }

    async fn get_opening_balance(
        &self,
        account_id: String,
        start_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Option<Decimal>, Error> {
        let prev_date = start_date - chrono::Duration::days(1);
        let balance = sqlx::query_scalar::<_, Decimal>(
            r#"
            SELECT current_balance
            FROM balance_snapshots
            WHERE account_id = $1
                AND tenant_id = $3
                AND snapshot_date = $2
            LIMIT 1
            "#,
        )
        .bind(&account_id)
        .bind(prev_date)
        .bind(tenant_id)
        .fetch_optional(self)
        .await?;

        Ok(balance)
    }

    async fn get_accounts(
        &self,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        let accounts = sqlx::query_as::<_, BankAccountWithLedger>(
            r#"
                select
                    b.payload->>'id' as id,
                    b.payload->>'account_number' as account_number,
                    b.payload->>'parent_id' as parent_id,
                    b.payload->>'status' as status,
                    b.payload->>'account_type' as account_type,
                    b.payload->>'kind' as kind,
                    b.payload->>'currency' as currency,
                    b.payload->>'ledger_id' as ledger_id,
                    (l.payload->'available'->>'amount')::numeric as available,
                    (l.payload->'pending'->>'amount')::numeric as pending,
                    (l.payload->'book_balance'->>'amount')::numeric as book_balance,
                    b.payload->>'created_at' as created_at,
                    b.payload->>'updated_at' as updated_at,
                    b.tenant_id
                from bank_account_views b
                left join ledger_views l on b.payload->>'ledger_id' = l.view_id
                where b.tenant_id = $1
                order by b.version desc
                offset $2 limit $3
            "#,
        )
        .bind(tenant_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(self)
        .await?;

        Ok(accounts)
    }

    async fn count_accounts(&self, tenant_id: i32) -> Result<i64, Error> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
                select count(*) from bank_account_views where tenant_id = $1
            "#,
        )
        .bind(tenant_id)
        .fetch_one(self)
        .await?;

        Ok(count)
    }
}
