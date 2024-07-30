use async_trait::async_trait;
use command::{BalanceCommand, BankAccountCommand};
use cqrs_es::Aggregate;
use event::{BaseEvent, Event};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::event_sourcing::*;
use crate::service::{BankAccountServices, MockBalanceServices};

#[async_trait]
impl Aggregate for models::BankAccount {
    type Command = command::BankAccountCommand;
    type Event = events::BankAccountEvent;
    type Error = error::BankAccountError;
    type Services = BankAccountServices;

    // This identifier should be unique to the system.
    fn aggregate_type() -> String {
        "bank_account".to_string()
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
            BankAccountCommand::ApproveAccount {
                account_id,
                ledger_id,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::BankAccountEvent::AccountKycApproved {
                    ledger_id: ledger_id.to_string(),
                    base_event,
                }])
            }
            BankAccountCommand::Deposit { amount } => {
                // TODO: need to create transaction first
                let command = BalanceCommand::Credit {
                    ledger_id: Uuid::parse_str(&self.ledger_id).unwrap(),
                    account_id: Uuid::parse_str(&self.id).unwrap(),
                    transaction_id: Uuid::new_v4(),
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
                // TODO: need to create transaction first
                let command = BalanceCommand::Debit {
                    ledger_id: Uuid::parse_str(&self.ledger_id).unwrap(),
                    account_id: Uuid::parse_str(&self.id).unwrap(),
                    transaction_id: Uuid::new_v4(),
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
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Pending;
                self.timestamp = base_event.get_created_at();
            }
            events::BankAccountEvent::AccountKycApproved {
                ledger_id,
                base_event,
            } => {
                self.id = base_event.get_aggregate_id();
                self.ledger_id = ledger_id;
                self.status = models::BankAccountStatus::Approved;
                self.timestamp = base_event.get_created_at();
            }
            // Money handling actions are just delegate to Ledger, does not need
            // to record anything in the event.
            events::BankAccountEvent::CustomerDepositedCash { .. } => {}
            events::BankAccountEvent::CustomerWithdrewCash { .. } => {}
        }
    }
}

#[async_trait]
impl Aggregate for models::Balance {
    type Command = command::BalanceCommand;
    type Event = events::BalanceEvent;
    type Error = error::LedgerError;
    type Services = MockBalanceServices;

    // This identifier should be unique to the system.
    fn aggregate_type() -> String {
        "balance".to_string()
    }

    // The aggregate logic goes here. Note that this will be the _bulk_ of a CQRS system
    // so expect to use helper functions elsewhere to keep the code clean.
    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            BalanceCommand::Debit {
                ledger_id,
                account_id,
                transaction_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(ledger_id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![
                    events::BalanceEvent::BalanceChanged {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "debit_hold".to_string(),
                        available_delta: Money::new(Decimal::ZERO - amount.amount, Currency::USD),
                        pending_delta: Money::new(amount.amount, Currency::USD),
                        base_event: base_event.clone(),
                    },
                    events::BalanceEvent::BalanceChanged {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "debit_release".to_string(),
                        available_delta: Money::new(Decimal::ZERO, Currency::USD),
                        pending_delta: Money::new(Decimal::ZERO - amount.amount, Currency::USD),
                        base_event,
                    },
                ])
            }
            BalanceCommand::Credit {
                ledger_id,
                account_id,
                transaction_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(ledger_id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![
                    events::BalanceEvent::BalanceChanged {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "credit_hold".to_string(),
                        available_delta: Money::new(Decimal::ZERO, Currency::USD),
                        pending_delta: Money::new(amount.amount, Currency::USD),
                        base_event: base_event.clone(),
                    },
                    events::BalanceEvent::BalanceChanged {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "credit_release".to_string(),
                        available_delta: Money::new(amount.amount, Currency::USD),
                        pending_delta: Money::new(Decimal::ZERO - amount.amount, Currency::USD),
                        base_event,
                    },
                ])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::BalanceEvent::BalanceChanged {
                amount,
                transaction_id: _,
                transaction_type: _,
                available_delta,
                pending_delta,
                base_event,
            } => {
                self.id = base_event.get_aggregate_id();
                self.amount = amount;
                self.available = self.available + available_delta;
                self.pending = self.pending + pending_delta;
                self.account_id = base_event.get_parent_id();
                self.timestamp = base_event.get_created_at();
            }
        }
    }
}

// The aggregate tests are the most important part of a CQRS system.
// The simplicity and flexibility of these tests are a good part of what
// makes an event sourced system so friendly to changing business requirements.
#[cfg(test)]
mod aggregate_tests {
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use std::sync::Mutex;
    use uuid::Uuid;

    use cqrs_es::test::TestFramework;

    use crate::{
        common::money::{Currency, Money},
        service::{BankAccountApi, BankAccountServices},
    };

    use super::{
        command::{BalanceCommand, BankAccountCommand},
        event::{BaseEvent, Event},
        events::BankAccountEvent,
        models::BankAccount,
    };

    // A test framework that will apply our events and command
    // and verify that the logic works as expected.
    type AccountTestFramework = TestFramework<BankAccount>;

    #[test]
    fn test_acccount_creation() {
        let uuid = Uuid::new_v4();
        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_created_at(chrono::Utc::now());

        let expected = BankAccountEvent::AccountOpened { base_event };
        let command = BankAccountCommand::OpenAccount { account_id: uuid };
        let services = BankAccountServices::new(Box::new(MockBankAccountServices::default()));
        // Obtain a new test framework
        AccountTestFramework::with(services)
            // In a test case with no previous events
            .given_no_previous_events()
            // Wnen we fire this command
            .when(command)
            // then we expect these results
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_acccount_kyc_approved() {
        let uuid = Uuid::new_v4();
        let ledger_id = Uuid::new_v4();

        let mut old_event = BaseEvent::default();
        old_event.set_aggregate_id(uuid);
        old_event.set_created_at(chrono::Utc::now());

        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_created_at(chrono::Utc::now());

        let previous = BankAccountEvent::AccountOpened {
            base_event: old_event,
        };
        let expected = BankAccountEvent::AccountKycApproved {
            ledger_id: ledger_id.to_string(),
            base_event,
        };
        let command = BankAccountCommand::ApproveAccount {
            account_id: uuid,
            ledger_id,
        };
        let mock_services = MockBankAccountServices::default();
        mock_services.set_write_ledger_response(Ok(()));
        let services = BankAccountServices::new(Box::new(mock_services));
        // Obtain a new test framework
        AccountTestFramework::with(services)
            .given(vec![previous])
            // Wnen we fire this command
            .when(command)
            // then we expect these results
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_deposit() {
        let uuid = Uuid::new_v4();
        let ledger_id = Uuid::new_v4();

        let mut old_event = BaseEvent::default();
        old_event.set_aggregate_id(uuid);
        old_event.set_created_at(chrono::Utc::now());

        let mut old_event2 = BaseEvent::default();
        old_event2.set_aggregate_id(uuid);
        old_event2.set_created_at(chrono::Utc::now());

        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_created_at(chrono::Utc::now());

        let previous_1 = BankAccountEvent::AccountOpened {
            base_event: old_event,
        };
        let previous_2 = BankAccountEvent::AccountKycApproved {
            ledger_id: ledger_id.to_string(),
            base_event: old_event2,
        };

        let command = BankAccountCommand::Deposit {
            amount: Money::new(dec!(1000.0), Currency::USD),
        };
        let mock_services = MockBankAccountServices::default();
        mock_services.set_write_ledger_response(Ok(()));
        let services = BankAccountServices::new(Box::new(mock_services));
        // Obtain a new test framework
        AccountTestFramework::with(services)
            .given(vec![previous_1, previous_2])
            // Wnen we fire this command
            .when(command)
            // then we expect these results
            .then_expect_events(vec![]);
    }

    pub struct MockBankAccountServices {
        write_ledger_response: Mutex<Option<Result<(), anyhow::Error>>>,
    }

    impl Default for MockBankAccountServices {
        fn default() -> Self {
            Self {
                write_ledger_response: Mutex::new(None),
            }
        }
    }

    impl MockBankAccountServices {
        fn set_write_ledger_response(&self, response: Result<(), anyhow::Error>) {
            *self.write_ledger_response.lock().unwrap() = Some(response);
        }
    }

    #[async_trait]
    impl BankAccountApi for MockBankAccountServices {
        async fn write_ledger(
            &self,
            _ledger_id: String,
            _command: BalanceCommand,
        ) -> Result<(), anyhow::Error> {
            self.write_ledger_response.lock().unwrap().take().unwrap()
        }
    }
}
