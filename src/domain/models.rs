use serde::{Deserialize, Serialize};

use crate::common::money::Money;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub enum BankAccountStatus {
    #[default]
    Pending,
    Approved,
    Freeze,
    CustomerClosed,
    Terminated,
}

#[derive(Serialize, Default, Deserialize)]
pub struct BankAccount {
    pub account_id: String,
    pub status: BankAccountStatus,
    pub ledger_id: String,
    pub timestamp: String,
}

// The view for a BankAccount query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BankAccountView {
    pub id: String,
    pub ledger_id: String,
    pub status: BankAccountStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub id: String,
    pub account_id: String,
    pub transaction_type: String,
    pub amount: Money,
    pub timestamp: String,
}

// The view for a Ledger query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LedgerView {
    pub id: String,
    pub account_id: Option<String>,
    pub available: Money,
    pub pending: Money,
    pub current: Money,
    pub transactions: Vec<Transaction>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub done_by: String,
    pub credit: Money,
    pub debit: Money,
    pub timestamp: String,
}

impl Transaction {
    pub fn new(done_by: &str, credit: Money, debit: Money, timestamp: String) -> Self {
        Self {
            done_by: done_by.to_string(),
            credit,
            debit,
            timestamp,
        }
    }
}
