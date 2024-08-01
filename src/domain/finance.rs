use chrono::NaiveDate;
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
    #[allow(dead_code)]
    pub journal_entry_id: Option<Uuid>,
}

#[derive(FromRow, Debug)]
pub struct JournalEntry {
    pub id: Uuid,
    pub entry_date: NaiveDate,
    pub description: Option<String>,
    pub status: String,
}

#[derive(FromRow, Debug)]
pub struct JournalLine {
    pub id: Uuid,
    #[allow(dead_code)]
    pub journal_entry_id: Option<Uuid>,
    pub ledger_id: Uuid,
    pub debit_amount: Decimal,
    pub credit_amount: Decimal,
    pub currency: String,
    pub description: Option<String>,
}
