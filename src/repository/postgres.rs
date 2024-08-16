use crate::common::money::Currency;
use crate::domain::finance::{JournalEntry, JournalLine, Transaction};
use crate::domain::models::{BankAccountKind, HouseAccount};
use crate::domain::tenant::Tenant;

use super::adapter::DatabaseClient;
use async_trait::async_trait;
use chrono::Local;
use sqlx::postgres::PgPool;
use sqlx::Error;
use uuid::Uuid;

#[async_trait]
impl DatabaseClient for PgPool {
    async fn fail_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE transactions
            SET status = 'failed', updated_at = NOW()
            WHERE id = $1
            "#,
            transaction_id,
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE transactions
            SET status = 'completed', updated_at = NOW()
            WHERE id = $1
            "#,
            transaction_id,
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, Error> {
        let mut tx = self.begin().await?;

        // Insert JournalEntry
        let journal_entry_id = sqlx::query!(
            r#"
            INSERT INTO journal_entries (id, entry_date, description, status)
            VALUES ($1, $2, $3, $4)
            RETURNING id
            "#,
            journal_entry.id,
            journal_entry.entry_date,
            journal_entry.description,
            journal_entry.status
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        // Insert JournalLines
        for journal_line in journal_lines {
            sqlx::query!(
                r#"
                INSERT INTO journal_lines (id, journal_entry_id, ledger_id, debit_amount, credit_amount, currency, description)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
                journal_line.id,
                journal_entry_id,
                journal_line.ledger_id,
                journal_line.debit_amount,
                journal_line.credit_amount,
                journal_line.currency,
                journal_line.description
            )
            .execute(&mut *tx)
            .await?;
        }

        // Insert Transaction
        let transaction_id = sqlx::query!(
            r#"
            INSERT INTO transactions (id, bank_account_id, transaction_reference,
            transaction_date, amount, currency, description, status, journal_entry_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id
            "#,
            transaction.id,
            transaction.bank_account_id,
            transaction.transaction_reference,
            transaction.transaction_date,
            transaction.amount,
            transaction.currency,
            transaction.description,
            transaction.status,
            journal_entry_id
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        tx.commit().await?;

        Ok(transaction_id)
    }

    async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error> {
        sqlx::query!(
            r#"
            INSERT INTO house_accounts (id, account_number, account_name, account_type, ledger_id, currency, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            account.id,
            account.account_number,
            account.account_name,
            account.account_type,
            account.ledger_id,
            account.currency.to_string(),
            account.status
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn get_house_account(&self, currency: Currency) -> Result<HouseAccount, Error> {
        let house_account = sqlx::query_as!(
            HouseAccount,
            r#"
            SELECT id, status, account_number, account_name, account_type, ledger_id, currency as "currency: String"
            FROM house_accounts
            WHERE currency = $1
            AND status = 'active'
            AND account_type = 'House'
            LIMIT 1
            "#,
            currency.to_string()
        )
        .fetch_one(self)
        .await?;

        Ok(house_account)
    }

    async fn get_house_accounts(&self, currency: Currency) -> Result<Vec<HouseAccount>, Error> {
        let house_accounts = sqlx::query_as!(
            HouseAccount,
            r#"
            SELECT id, status, account_number, account_name, account_type, ledger_id, currency as "currency: String"
            FROM house_accounts
            WHERE currency = $1
            AND status = 'active'
            "#,
            currency.to_string()
        )
        .fetch_all(self)
        .await?;

        Ok(house_accounts)
    }

    async fn validate_bank_account_exists(
        &self,
        user_id: String,
        currency: Currency,
        kind: BankAccountKind,
    ) -> Result<bool, Error> {
        let count = sqlx::query!(
            r#"
            select count(1) as total from bank_account_views
            where payload->>'user_id'=$1
            and payload->>'currency'=$2
            and payload->>'kind'=$3
            and payload->>'status' IN ('Pending', 'Approved', 'Freeze');
            "#,
            user_id,
            currency.to_string(),
            kind.to_string()
        )
        .fetch_one(self)
        .await?
        .total;

        if count.map_or(false, |value| value != 0) {
            Ok(false)
        } else {
            Ok(true)
        }
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
}
