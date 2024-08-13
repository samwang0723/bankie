use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::common::money::{Currency, Money};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BankAccountStatus {
    #[default]
    Pending,
    Approved,
    Freeze,
    CustomerClosed,
    Terminated,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BankAccountType {
    #[default]
    Retail,
    Institution,
    Tax,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BankAccountKind {
    #[default]
    Checking,
    Interest,
    Yield,
}

impl fmt::Display for BankAccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankAccountKind::Checking => write!(f, "Checking"),
            BankAccountKind::Interest => write!(f, "Interest"),
            BankAccountKind::Yield => write!(f, "Yield"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LedgerAction {
    #[default]
    Deposit,
    Withdraw,
}

#[derive(Serialize, Default, Deserialize)]
pub struct BankAccount {
    pub id: String,
    pub status: BankAccountStatus,
    pub account_type: BankAccountType,
    pub kind: BankAccountKind,
    pub currency: Currency,
    pub ledger_id: String,
    pub user_id: String,
    pub timestamp: String,
}

#[derive(Serialize, Default, Deserialize)]
pub struct HouseAccount {
    pub id: Uuid,
    pub status: String,
    #[serde(skip_deserializing)]
    pub account_number: String,
    pub account_name: String,
    pub account_type: String,
    #[serde(skip_deserializing)]
    pub ledger_id: String,
    pub currency: Currency,
}

// The view for a BankAccount query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BankAccountView {
    pub id: String,
    pub ledger_id: String,
    pub user_id: String,
    pub status: BankAccountStatus,
    pub account_type: BankAccountType,
    pub kind: BankAccountKind,
    pub currency: Currency,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub id: String,
    pub account_id: String,
    pub available: Money,
    pub pending: Money,
    pub amount: Money,
    pub timestamp: String,
}

// The view for a Ledger query
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LedgerView {
    pub id: String,
    pub account_id: String,
    pub available: Money,
    pub pending: Money,
    pub current: Money,
    pub created_at: String,
    pub updated_at: String,
}
