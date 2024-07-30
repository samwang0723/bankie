use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::{common::money::Money, event_sourcing::event::BaseEvent};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BankAccountEvent {
    AccountOpened {
        base_event: BaseEvent,
    },
    AccountKycApproved {
        ledger_id: String,
        base_event: BaseEvent,
    },
    CustomerDepositedMoney {
        amount: Money,
        ledger_id: String,
        base_event: BaseEvent,
    },
    CustomerWithdrewCash {
        amount: Money,
        ledger_id: String,
        base_event: BaseEvent,
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
        amount: Money,
        base_event: BaseEvent,
    },
    LedgerDebited {
        amount: Money,
        base_event: BaseEvent,
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
