use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::prelude::FromRow;
use uuid::Uuid;

use crate::common::money::Money;

use super::models::LedgerAction;

pub const TRANS_DEPOSIT: &str = "DE";
pub const TRANS_WITHDRAWAL: &str = "WI";

#[derive(FromRow, Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct TransactionWithMoney {
    pub id: Uuid,
    pub bank_account_id: Uuid,
    pub transaction_reference: String,
    pub transaction_date: NaiveDate,
    pub amount: String,
    pub currency: String,
    pub description: Option<String>,
    pub metadata: Value,
    pub status: String,
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

impl Transaction {
    pub fn into_transaction_with_money(self) -> TransactionWithMoney {
        TransactionWithMoney {
            id: self.id,
            bank_account_id: self.bank_account_id,
            transaction_reference: self.transaction_reference,
            transaction_date: self.transaction_date,
            amount: format!("{}", Money::new(self.amount, self.currency.clone().into())),
            currency: self.currency,
            description: self.description,
            metadata: self.metadata,
            status: self.status,
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
