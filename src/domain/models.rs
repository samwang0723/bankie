use serde::{Deserialize, Serialize};

use crate::common::money::Money;

#[derive(Serialize, Default, Deserialize)]
pub struct BankAccount {
    pub account_id: String,
    pub balance: Money,
}

// The view for a BankAccount query, for a standard http application this should
// be designed to reflect the response dto that will be returned to a user.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BankAccountView {
    pub account_id: Option<String>,
    pub balance: Money,
    pub written_checks: Vec<String>,
    pub ledger: Vec<LedgerEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub description: String,
    pub amount: Money,
}

impl LedgerEntry {
    pub fn new(description: &str, amount: Money) -> Self {
        Self {
            description: description.to_string(),
            amount,
        }
    }
}
