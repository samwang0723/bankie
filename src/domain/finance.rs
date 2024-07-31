use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::prelude::FromRow;
use uuid::Uuid;

#[derive(FromRow, Debug)]
pub struct Transaction {
    pub id: Uuid,
    pub bank_account_id: Uuid,
    pub transaction_reference: String,
    pub transaction_date: NaiveDate,
    pub amount: Decimal,
    pub currency: String,
    pub description: Option<String>,
    pub status: String,
    pub journal_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow, Debug)]
pub struct JournalEntry {
    pub id: Uuid,
    pub entry_date: NaiveDate,
    pub description: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow, Debug)]
pub struct JournalLine {
    pub id: Uuid,
    pub journal_entry_id: Uuid,
    pub balance_id: Uuid,
    pub debit_amount: Decimal,
    pub credit_amount: Decimal,
    pub currency: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
