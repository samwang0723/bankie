use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::common::money::Money;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BankAccountEvent {
    AccountOpened {
        account_id: String,
        timestamp: String,
    },
    AccountKycApproved {
        account_id: String,
        ledger_id: String,
        timestamp: String,
    },
    CustomerDepositedMoney {
        amount: Money,
    },
    CustomerWithdrewCash {
        amount: Money,
    },
}

impl DomainEvent for BankAccountEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            BankAccountEvent::AccountOpened { .. } => "AccountOpened",
            BankAccountEvent::AccountKycApproved { .. } => "AccountKycApproved",
            BankAccountEvent::CustomerDepositedMoney { .. } => "CustomerDepositedMoney",
            BankAccountEvent::CustomerWithdrewCash { .. } => "CustomerWithdrewCash",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LedgerEvent {
    LedgerCredited {
        ledger_id: String,
        account_id: String,
        amount: Money,
        timestamp: String,
    },
    LedgerDebited {
        ledger_id: String,
        account_id: String,
        amount: Money,
        timestamp: String,
    },
}

impl DomainEvent for LedgerEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            LedgerEvent::LedgerCredited { .. } => "LedgerCredited",
            LedgerEvent::LedgerDebited { .. } => "LedgerDebited",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
