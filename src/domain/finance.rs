use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use sqlx::prelude::FromRow;
use uuid::Uuid;

use crate::common::money::default_precision;

use super::models::LedgerAction;

pub const TRANS_DEPOSIT: &str = "DE";
pub const TRANS_WITHDRAWAL: &str = "WI";
pub const TRANS_TRANSFER: &str = "TR";

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
        if self.transaction_reference.contains(TRANS_DEPOSIT) {
            LedgerAction::Deposit
        } else if self.transaction_reference.contains(TRANS_WITHDRAWAL) {
            LedgerAction::Withdraw
        } else if self.transaction_reference.contains(TRANS_TRANSFER) {
            LedgerAction::Transfer
        } else {
            LedgerAction::Deposit // safe fallback instead of panic
        }
    }

    pub fn into_transaction_with_money(self) -> TransactionWithMoney {
        let precision = default_precision(&self.currency) as usize;
        let amount_str = format!("{:.prec$}", self.amount, prec = precision);
        TransactionWithMoney {
            id: self.id,
            bank_account_id: self.bank_account_id,
            transaction_reference: self.transaction_reference,
            transaction_date: self.transaction_date,
            amount: amount_str,
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
    pub id: i32,
    pub transaction_id: Uuid,
    pub event_type: String,
    pub payload: Value,
    #[allow(dead_code)]
    pub processed: bool,
    pub retry_count: i32,
}

#[derive(FromRow, Debug, Serialize)]
pub struct BalanceSnapshot {
    pub id: Uuid,
    pub tenant_id: i32,
    pub account_id: String,
    pub ledger_id: String,
    pub asset_code: String,
    pub available: Decimal,
    pub pending: Decimal,
    pub current_balance: Decimal,
    pub snapshot_date: NaiveDate,
}
