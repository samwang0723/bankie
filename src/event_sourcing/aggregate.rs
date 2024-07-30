use async_trait::async_trait;
use command::{BankAccountCommand, LedgerCommand};
use cqrs_es::Aggregate;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::event_sourcing::*;
use crate::service::{BankAccountServices, LedgerType, MockLedgerServices};

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
                Ok(vec![events::BankAccountEvent::AccountOpened {
                    account_id,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }])
            }
            BankAccountCommand::ApproveAccount { account_id } => {
                let ledger_id = Uuid::new_v4().to_string();
                if services
                    .services
                    .write_ledger(
                        &ledger_id,
                        &account_id,
                        Money::new(Decimal::ZERO, Currency::USD),
                        LedgerType::Init,
                    )
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };

                Ok(vec![events::BankAccountEvent::AccountKycApproved {
                    account_id,
                    ledger_id,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                }])
            }
            BankAccountCommand::Deposit { amount } => {
                if services
                    .services
                    .write_ledger(
                        &self.ledger_id,
                        &self.account_id,
                        amount,
                        LedgerType::Credit,
                    )
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };
                Ok(vec![])
            }
            BankAccountCommand::Withdrawl { amount } => {
                if services
                    .services
                    .write_ledger(&self.ledger_id, &self.account_id, amount, LedgerType::Debit)
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
            events::BankAccountEvent::AccountOpened {
                account_id,
                timestamp,
            } => {
                self.account_id = account_id;
                self.status = models::BankAccountStatus::Pending;
                self.timestamp = timestamp;
            }
            events::BankAccountEvent::AccountKycApproved {
                account_id,
                ledger_id,
                timestamp,
            } => {
                self.account_id = account_id;
                self.ledger_id = ledger_id;
                self.status = models::BankAccountStatus::Approved;
                self.timestamp = timestamp;
            }
            // Money handling actions are just delegate to Ledger, does not need
            // to record anything in the event.
            events::BankAccountEvent::CustomerDepositedMoney { amount: _ } => {}
            events::BankAccountEvent::CustomerWithdrewCash { amount: _ } => {}
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
            } => Ok(vec![events::LedgerEvent::LedgerCredited {
                ledger_id,

                account_id,
                amount: Money::new(Decimal::ZERO, Currency::USD),
                timestamp: chrono::Utc::now().to_rfc3339(),
            }]),
            LedgerCommand::Debit {
                ledger_id,
                account_id,
                amount,
            } => Ok(vec![events::LedgerEvent::LedgerDebited {
                ledger_id,
                account_id,
                amount,
                timestamp: chrono::Utc::now().to_rfc3339(),
            }]),
            LedgerCommand::Credit {
                ledger_id,
                account_id,
                amount,
            } => Ok(vec![events::LedgerEvent::LedgerCredited {
                ledger_id,
                account_id,
                amount,
                timestamp: chrono::Utc::now().to_rfc3339(),
            }]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::LedgerEvent::LedgerCredited {
                ledger_id,
                account_id,
                amount,
                timestamp,
            } => {
                self.id = ledger_id;
                self.amount = amount;
                self.transaction_type = LedgerType::Credit.to_string();
                self.account_id = account_id;
                self.timestamp = timestamp;
            }
            events::LedgerEvent::LedgerDebited {
                ledger_id,

                account_id,
                amount,
                timestamp,
            } => {
                self.id = ledger_id;
                self.amount = amount;
                self.transaction_type = LedgerType::Debit.to_string();
                self.account_id = account_id;
                self.timestamp = timestamp;
            }
        }
    }
}
