use async_trait::async_trait;
use command::{BankAccountCommand, LedgerCommand};
use cqrs_es::{Aggregate, DomainEvent};
use event::{BaseEvent, Event};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::event_sourcing::*;
use crate::service::{BankAccountServices, MockLedgerServices};

#[async_trait]
impl Aggregate for models::BankAccount {
    type Command = command::BankAccountCommand;
    type Event = events::BankAccountEvent;
    type Error = error::BankAccountError;
    type Services = BankAccountServices;

    // This identifier should be unique to the system.
    fn aggregate_type() -> String {
        "Account".to_string()
    }

    // The aggregate logic goes here. Note that this will be the _bulk_ of a CQRS system
    // so expect to use helper functions elsewhere to keep the code clean.
    async fn handle(
        &self,
        command: Self::Command,
        services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            BankAccountCommand::OpenAccount { account_id } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::BankAccountEvent::AccountOpened { base_event }])
            }
            BankAccountCommand::ApproveAccount { account_id } => {
                let ledger_id = Uuid::new_v4();
                let command = LedgerCommand::Init {
                    ledger_id,
                    account_id,
                };
                if services
                    .services
                    .write_ledger(ledger_id.to_string(), command)
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::BankAccountEvent::AccountKycApproved {
                    ledger_id: ledger_id.to_string(),
                    base_event,
                }])
            }
            BankAccountCommand::Deposit { amount } => {
                let command = LedgerCommand::Credit {
                    ledger_id: Uuid::parse_str(&self.ledger_id).unwrap(),
                    account_id: Uuid::parse_str(&self.account_id).unwrap(),
                    amount,
                };
                if services
                    .services
                    .write_ledger(self.ledger_id.clone(), command)
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };
                Ok(vec![])
            }
            BankAccountCommand::Withdrawl { amount } => {
                let command = LedgerCommand::Debit {
                    ledger_id: Uuid::parse_str(&self.ledger_id).unwrap(),
                    account_id: Uuid::parse_str(&self.account_id).unwrap(),
                    amount,
                };
                if services
                    .services
                    .write_ledger(self.ledger_id.clone(), command)
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };
                Ok(vec![])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::BankAccountEvent::AccountOpened { base_event } => {
                self.account_id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Pending;
                self.timestamp = base_event.get_created_at();
            }
            events::BankAccountEvent::AccountKycApproved {
                ledger_id,
                base_event,
            } => {
                self.account_id = base_event.get_aggregate_id();
                self.ledger_id = ledger_id;
                self.status = models::BankAccountStatus::Approved;
                self.timestamp = base_event.get_created_at();
            }
            // Money handling actions are just delegate to Ledger, does not need
            // to record anything in the event.
            events::BankAccountEvent::CustomerDepositedMoney { .. } => {}
            events::BankAccountEvent::CustomerWithdrewCash { .. } => {}
        }
    }
}

#[async_trait]
impl Aggregate for models::Ledger {
    type Command = command::LedgerCommand;
    type Event = events::LedgerEvent;
    type Error = error::LedgerError;
    type Services = MockLedgerServices;

    // This identifier should be unique to the system.
    fn aggregate_type() -> String {
        "Ledger".to_string()
    }

    // The aggregate logic goes here. Note that this will be the _bulk_ of a CQRS system
    // so expect to use helper functions elsewhere to keep the code clean.
    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            LedgerCommand::Init {
                ledger_id,
                account_id,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(ledger_id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerCredited {
                    amount: Money::new(Decimal::ZERO, Currency::USD),
                    base_event,
                }])
            }
            LedgerCommand::Debit {
                ledger_id,
                account_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(ledger_id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerDebited {
                    amount,
                    base_event,
                }])
            }
            LedgerCommand::Credit {
                ledger_id,
                account_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(ledger_id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerCredited {
                    amount,
                    base_event,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        let event_type = event.event_type();
        match event {
            events::LedgerEvent::LedgerCredited { amount, base_event } => {
                self.id = base_event.get_aggregate_id();
                self.amount = amount;
                self.transaction_type = event_type;
                self.account_id = base_event.get_parent_id();
                self.timestamp = base_event.get_created_at();
            }
            events::LedgerEvent::LedgerDebited { amount, base_event } => {
                self.id = base_event.get_aggregate_id();
                self.amount = amount;
                self.transaction_type = event_type;
                self.account_id = base_event.get_parent_id();
                self.timestamp = base_event.get_created_at();
            }
        }
    }
}
