use crate::common::money::Currency;
use crate::domain::finance::{JournalEntry, JournalLine, Transaction};
use crate::domain::models::HouseAccount;

use super::adapter::DatabaseClient;
use async_trait::async_trait;
use sqlx::postgres::PgPool;
use sqlx::Error;
use uuid::Uuid;

#[async_trait]
impl DatabaseClient for PgPool {
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

    async fn get_house_account(&self, currency: Currency) -> Result<String, Error> {
        let house_account_id = sqlx::query!(
            r#"
            SELECT ledger_id
            FROM house_accounts
            WHERE currency = $1
            "#,
            currency.to_string()
        )
        .fetch_one(self)
        .await?
        .ledger_id;

        Ok(house_account_id)
    }
}
