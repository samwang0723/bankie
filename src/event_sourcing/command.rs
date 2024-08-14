use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    common::money::{Currency, Money},
    domain::models::{BankAccountKind, BankAccountType},
};

#[derive(Debug, Serialize, Deserialize)]
pub enum BankAccountCommand {
    OpenAccount {
        #[serde(skip_deserializing)]
        id: Uuid,
        account_type: BankAccountType,
        kind: BankAccountKind,
        user_id: String,
        currency: Currency,
    },
    ApproveAccount {
        id: Uuid,
        #[serde(skip_deserializing)]
        ledger_id: Uuid,
    },
    Deposit {
        id: Uuid,
        amount: Money,
    },
    Withdrawl {
        id: Uuid,
        amount: Money,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LedgerCommand {
    Init {
        id: Uuid,
        account_id: Uuid,
        amount: Money,
    },
    Credit {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
    Debit {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
}
