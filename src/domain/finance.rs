use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::prelude::FromRow;
use uuid::Uuid;

use super::models::LedgerAction;

pub const TRANS_DEPOSIT: &str = "DE";
pub const TRANS_WITHDRAWAL: &str = "WI";

#[derive(FromRow, Debug)]
pub struct Transaction {
    pub id: Uuid,
    pub bank_account_id: Uuid,
    pub transaction_reference: String,
    pub transaction_date: NaiveDate,
    pub amount: Decimal,
    pub currency: String,
    pub description: Option<String>,
    pub metadata: Value,
    pub status: String,
    #[allow(dead_code)]
    pub journal_entry_id: Option<Uuid>,
}

impl Transaction {
    pub fn transaction_type(&self) -> LedgerAction {
        // if transaction_reference contains DE / WI
        if self.transaction_reference.contains(TRANS_DEPOSIT) {
            LedgerAction::Deposit
        } else if self.transaction_reference.contains(TRANS_WITHDRAWAL) {
            LedgerAction::Withdraw
        } else {
            panic!("Invalid transaction type");
        }
    }
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
    pub ledger_id: String,
    pub debit_amount: Decimal,
    pub credit_amount: Decimal,
    pub currency: String,
    pub description: Option<String>,
}

#[derive(FromRow, Debug)]
pub struct Outbox {
    #[allow(dead_code)]
    pub id: i32,
    #[allow(dead_code)]
    pub transaction_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    #[allow(dead_code)]
    pub processed: bool,
}
