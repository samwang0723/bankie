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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    fn make_transaction(reference: &str, amount: Decimal, currency: &str) -> Transaction {
        Transaction {
            id: Uuid::new_v4(),
            bank_account_id: Uuid::new_v4(),
            transaction_reference: reference.to_string(),
            transaction_date: NaiveDate::from_ymd_opt(2025, 1, 15).unwrap(),
            amount,
            currency: currency.to_string(),
            description: Some("test".to_string()),
            metadata: json!({}),
            status: "completed".to_string(),
            journal_entry_id: None,
        }
    }

    #[test]
    fn test_transaction_type_deposit() {
        let tx = make_transaction("DE-12345", dec!(100), "USD");
        assert_eq!(tx.transaction_type(), LedgerAction::Deposit);
    }

    #[test]
    fn test_transaction_type_withdrawal() {
        let tx = make_transaction("WI-67890", dec!(50), "USD");
        assert_eq!(tx.transaction_type(), LedgerAction::Withdraw);
    }

    #[test]
    fn test_transaction_type_transfer() {
        let tx = make_transaction("TR-11111", dec!(75), "USD");
        assert_eq!(tx.transaction_type(), LedgerAction::Transfer);
    }

    #[test]
    fn test_transaction_type_unknown_fallback() {
        let tx = make_transaction("XX-99999", dec!(10), "USD");
        assert_eq!(tx.transaction_type(), LedgerAction::Deposit); // safe fallback
    }

    #[test]
    fn test_into_transaction_with_money_usd() {
        let tx = make_transaction("DE-100", dec!(1234.56), "USD");
        let with_money = tx.into_transaction_with_money();
        assert_eq!(with_money.amount, "1234.56"); // USD precision = 2
        assert_eq!(with_money.currency, "USD");
    }

    #[test]
    fn test_into_transaction_with_money_twd() {
        let tx = make_transaction("DE-101", dec!(5000), "TWD");
        let with_money = tx.into_transaction_with_money();
        assert_eq!(with_money.amount, "5000"); // TWD precision = 0
    }

    #[test]
    fn test_into_transaction_with_money_btc() {
        let tx = make_transaction("TR-200", dec!(0.00123456), "BTC");
        let with_money = tx.into_transaction_with_money();
        assert_eq!(with_money.amount, "0.00123456"); // BTC precision = 8
    }

    #[test]
    fn test_into_transaction_with_money_preserves_fields() {
        let original_id = Uuid::new_v4();
        let account_id = Uuid::new_v4();
        let tx = Transaction {
            id: original_id,
            bank_account_id: account_id,
            transaction_reference: "DE-999".to_string(),
            transaction_date: NaiveDate::from_ymd_opt(2025, 6, 1).unwrap(),
            amount: dec!(100),
            currency: "USD".to_string(),
            description: Some("Test deposit".to_string()),
            metadata: json!({"key": "value"}),
            status: "posted".to_string(),
            journal_entry_id: Some(Uuid::new_v4()),
        };
        let with_money = tx.into_transaction_with_money();
        assert_eq!(with_money.id, original_id);
        assert_eq!(with_money.bank_account_id, account_id);
        assert_eq!(with_money.transaction_reference, "DE-999");
        assert_eq!(with_money.description, Some("Test deposit".to_string()));
        assert_eq!(with_money.status, "posted");
    }
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
