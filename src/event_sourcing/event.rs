use cqrs_es::DomainEvent;
use serde::{Deserialize, Serialize};

use crate::common::money::Money;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BankAccountEvent {
    AccountOpened {
        account_id: String,
    },
    CustomerDepositedMoney {
        amount: Money,
        balance: Money,
    },
    CustomerWithdrewCash {
        amount: Money,
        balance: Money,
    },
    CustomerWroteCheck {
        check_number: String,
        amount: Money,
        balance: Money,
    },
}

impl DomainEvent for BankAccountEvent {
    fn event_type(&self) -> String {
        let event_type: &str = match self {
            BankAccountEvent::AccountOpened { .. } => "AccountOpened",
            BankAccountEvent::CustomerDepositedMoney { .. } => "CustomerDepositedMoney",
            BankAccountEvent::CustomerWithdrewCash { .. } => "CustomerWithdrewCash",
            BankAccountEvent::CustomerWroteCheck { .. } => "CustomerWroteCheck",
        };
        event_type.to_string()
    }

    fn event_version(&self) -> String {
        "1.0".to_string()
    }
}
