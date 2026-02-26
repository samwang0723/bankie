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
        #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
        parent_id: Option<Uuid>,
        #[serde(skip_deserializing)]
        account_number: String,
        account_type: BankAccountType,
        kind: BankAccountKind,
        #[serde(default, alias = "user_id")]
        external_reference_id: Option<String>,
        currency: Currency,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    ApproveAccount {
        id: Uuid,
        #[serde(skip_deserializing)]
        ledger_id: Uuid,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    FreezeAccount {
        id: Uuid,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    UnfreezeAccount {
        id: Uuid,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    CloseAccount {
        id: Uuid,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    Deposit {
        id: Uuid,
        amount: Money,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    Withdrawal {
        id: Uuid,
        amount: Money,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
    Transfer {
        id: Uuid,
        to_account_id: Uuid,
        amount: Money,
        #[serde(skip_deserializing, default)]
        tenant_id: i32,
    },
}

impl BankAccountCommand {
    /// Set tenant_id on any command variant (called by CommandExtractor after deserialization).
    pub fn set_tenant_id(&mut self, tid: i32) {
        match self {
            BankAccountCommand::OpenAccount { tenant_id, .. }
            | BankAccountCommand::ApproveAccount { tenant_id, .. }
            | BankAccountCommand::FreezeAccount { tenant_id, .. }
            | BankAccountCommand::UnfreezeAccount { tenant_id, .. }
            | BankAccountCommand::CloseAccount { tenant_id, .. }
            | BankAccountCommand::Deposit { tenant_id, .. }
            | BankAccountCommand::Withdrawal { tenant_id, .. }
            | BankAccountCommand::Transfer { tenant_id, .. } => *tenant_id = tid,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum LedgerCommand {
    Init {
        id: Uuid,
        account_id: Uuid,
        amount: Money,
        #[serde(default)]
        tenant_id: i32,
    },
    Credit {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
        #[serde(default)]
        tenant_id: i32,
    },
    DebitHold {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
        #[serde(default)]
        tenant_id: i32,
    },
    DebitRelease {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
        #[serde(default)]
        tenant_id: i32,
    },
}
