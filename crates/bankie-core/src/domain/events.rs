use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::{
    common::money::{Currency, Money},
    event_sourcing::event::BaseEvent,
};

use super::models::{BankAccountKind, BankAccountType};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BankAccountEvent {
    AccountOpened {
        account_type: BankAccountType,
        kind: BankAccountKind,
        currency: Currency,
        #[serde(default, alias = "user_id")]
        external_reference_id: Option<String>,
        #[serde(default)]
        account_number: String,
        #[serde(default)]
        name: Option<String>,
        base_event: BaseEvent,
    },
    AccountKycApproved {
        ledger_id: String,
        base_event: BaseEvent,
    },
    AccountFrozen {
        base_event: BaseEvent,
    },
    AccountUnfrozen {
        base_event: BaseEvent,
    },
    AccountClosed {
        base_event: BaseEvent,
    },
    CustomerDepositedCash {
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
            BankAccountEvent::AccountOpened { .. } => "bank_account.opened",
            BankAccountEvent::AccountKycApproved { .. } => "bank_account.kyc_approved",
            BankAccountEvent::AccountFrozen { .. } => "bank_account.frozen",
            BankAccountEvent::AccountUnfrozen { .. } => "bank_account.unfrozen",
            BankAccountEvent::AccountClosed { .. } => "bank_account.closed",
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
pub enum LedgerEvent {
    LedgerInitiated {
        amount: Money,
        base_event: BaseEvent,
    },
    LedgerUpdated {
        amount: Money,
        transaction_id: String,
        transaction_type: String,
        available_delta: Money,
        pending_delta: Money,
        base_event: BaseEvent,
    },
}

impl DomainEvent for LedgerEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            LedgerEvent::LedgerInitiated { .. } => "ledger.initiated",
            LedgerEvent::LedgerUpdated { .. } => "ledger.updated",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
