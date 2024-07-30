use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::{common::money::Money, event_sourcing::event::BaseEvent};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BankAccountEvent {
    AccountOpened {
        base_event: BaseEvent,
    },
    AccountKycApproved {
        balance_id: String,
        base_event: BaseEvent,
    },
    CustomerDepositedCash {
        amount: Money,
        balance_id: String,
        base_event: BaseEvent,
    },
    CustomerWithdrewCash {
        amount: Money,
        balance_id: String,
        base_event: BaseEvent,
    },
}

impl DomainEvent for BankAccountEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            BankAccountEvent::AccountOpened { .. } => "bank_account.opened",
            BankAccountEvent::AccountKycApproved { .. } => "bank_account.kyc_approved",
            BankAccountEvent::CustomerDepositedCash { .. } => "bank_account.deposited",
            BankAccountEvent::CustomerWithdrewCash { .. } => "bank_account.withdrew",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BalanceEvent {
    BalanceInitiated {
        base_event: BaseEvent,
    },
    BalanceChanged {
        amount: Money,
        transaction_id: String,
        transaction_type: String,
        available_delta: Money,
        pending_delta: Money,
        base_event: BaseEvent,
    },
}

impl DomainEvent for BalanceEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            BalanceEvent::BalanceInitiated { .. } => "balance.initiated",
            BalanceEvent::BalanceChanged { .. } => "balance.changed",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
