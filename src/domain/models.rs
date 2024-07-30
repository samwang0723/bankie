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
    pub id: String,
    pub status: BankAccountStatus,
    pub balance_id: String,
    pub timestamp: String,
}

// The view for a BankAccount query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BankAccountView {
    pub id: String,
    pub balance_id: String,
    pub status: BankAccountStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Balance {
    pub id: String,
    pub account_id: String,
    pub available: Money,
    pub pending: Money,
    pub amount: Money,
    pub timestamp: String,
}

// The view for a Ledger query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BalanceView {
    pub id: String,
    pub account_id: String,
    pub available: Money,
    pub pending: Money,
    pub current_balance: Money,
    pub created_at: String,
    pub updated_at: String,
}
