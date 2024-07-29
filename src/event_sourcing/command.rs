use serde::Deserialize;

use crate::common::money::Money;

#[derive(Debug, Deserialize)]
pub enum BankAccountCommand {
    OpenAccount { account_id: String },
    ApproveAccount { account_id: String },
    Deposit { amount: Money },
    Withdrawl { amount: Money },
}

#[derive(Debug, Deserialize)]
pub enum LedgerCommand {
    Init {
        ledger_id: String,
        account_id: String,
    },
    Credit {
        ledger_id: String,
        account_id: String,
        amount: Money,
    },
    Debit {
        ledger_id: String,
        account_id: String,
        amount: Money,
    },
}
