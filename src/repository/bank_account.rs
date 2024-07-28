use async_trait::async_trait;
use commands::BankAccountCommand;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{Aggregate, CqrsFramework, EventEnvelope, Query, View};
use events::BankAccountEvent;
use postgres_es::PostgresViewRepository;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::common::money::{Currency, Money};
use crate::domain::models::BankAccount;
use crate::event_sourcing::*;
use crate::service::BankAccountServices;

#[async_trait]
impl Aggregate for BankAccount {
    type Command = commands::BankAccountCommand;
    type Event = events::BankAccountEvent;
    type Error = errors::BankAccountError;
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
            BankAccountCommand::DepositMoney { amount } => {
                let balance = self.balance + amount;
                Ok(vec![BankAccountEvent::CustomerDepositedMoney {
                    amount,
                    balance,
                }])
            }
            BankAccountCommand::WithdrawMoney { amount } => {
                let balance = self.balance - amount;
                if balance < Money::new(Decimal::ZERO, Currency::USD) {
                    return Err("insufficient funds".into());
                }
                Ok(vec![BankAccountEvent::CustomerWithdrewCash {
                    amount,
                    balance,
                }])
            }
            BankAccountCommand::WriteCheck {
                check_number,
                amount,
            } => {
                let balance = self.balance - amount;
                if balance < Money::new(Decimal::ZERO, Currency::USD) {
                    return Err("funds not available".into());
                }

                Ok(vec![BankAccountEvent::CustomerWroteCheck {
                    check_number,
                    amount,
                    balance,
                }])
            }
            _ => Ok(vec![]),
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::BankAccountEvent::AccountOpened { .. } => self.opened = true,

            events::BankAccountEvent::CustomerDepositedMoney { amount: _, balance } => {
                self.balance = balance;
            }

            events::BankAccountEvent::CustomerWithdrewCash { amount: _, balance } => {
                self.balance = balance;
            }

            events::BankAccountEvent::CustomerWroteCheck {
                check_number: _,
                amount: _,
                balance,
            } => {
                self.balance = balance;
            }
        }
    }
}

pub struct SimpleLoggingQuery {}

#[async_trait]
impl Query<BankAccount> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<BankAccount>]) {
        for event in events {
            println!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
        }
    }
}

// Our second query, this one will be handled with Postgres `GenericQuery`
// which will serialize and persist our view after it is updated. It also
// provides a `load` method to deserialize the view on request.
pub type AccountQuery = GenericQuery<
    PostgresViewRepository<BankAccountView, BankAccount>,
    BankAccountView,
    BankAccount,
>;

// The view for a BankAccount query, for a standard http application this should
// be designed to reflect the response dto that will be returned to a user.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BankAccountView {
    account_id: Option<String>,
    balance: Money,
    written_checks: Vec<String>,
    ledger: Vec<LedgerEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LedgerEntry {
    description: String,
    amount: Money,
}
impl LedgerEntry {
    fn new(description: &str, amount: Money) -> Self {
        Self {
            description: description.to_string(),
            amount,
        }
    }
}

// This updates the view with events as they are committed.
// The logic should be minimal here, e.g., don't calculate the account balance,
// design the events to carry the balance information instead.
impl View<BankAccount> for BankAccountView {
    fn update(&mut self, event: &EventEnvelope<BankAccount>) {
        match &event.payload {
            BankAccountEvent::AccountOpened { account_id } => {
                self.account_id = Some(account_id.clone());
            }

            BankAccountEvent::CustomerDepositedMoney { amount, balance } => {
                self.ledger.push(LedgerEntry::new("deposit", *amount));
                self.balance = *balance;
            }

            BankAccountEvent::CustomerWithdrewCash { amount, balance } => {
                self.ledger
                    .push(LedgerEntry::new("atm withdrawal", *amount));
                self.balance = *balance;
            }

            BankAccountEvent::CustomerWroteCheck {
                check_number,
                amount,
                balance,
            } => {
                self.ledger.push(LedgerEntry::new(check_number, *amount));
                self.written_checks.push(check_number.clone());
                self.balance = *balance;
            }
        }
    }
}

#[cfg(test)]
mod aggregate_tests {
    use crate::service::{AtmError, BankAccountApi, CheckingError};

    use super::*;
    use cqrs_es::test::TestFramework;
    use events::BankAccountEvent;
    use rust_decimal_macros::dec;
    use std::sync::Mutex;

    type AccountTestFramework = TestFramework<BankAccount>;

    #[test]
    fn test_deposit_money() {
        let expected = BankAccountEvent::CustomerDepositedMoney {
            amount: Money::new(dec!(200.00), Currency::USD),
            balance: Money::new(dec!(200.00), Currency::USD),
        };

        let services = BankAccountServices::new(Box::new(MockBankAccountServices::default()));
        AccountTestFramework::with(services)
            .given_no_previous_events()
            .when(BankAccountCommand::DepositMoney {
                amount: Money::new(dec!(200.00), Currency::USD),
            })
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_withdraw_money() {
        let previous = BankAccountEvent::CustomerDepositedMoney {
            amount: Money::new(dec!(200.00), Currency::USD),
            balance: Money::new(dec!(200.00), Currency::USD),
        };
        let expected = BankAccountEvent::CustomerWithdrewCash {
            amount: Money::new(dec!(100.00), Currency::USD),
            balance: Money::new(dec!(100.00), Currency::USD),
        };
        let services = BankAccountServices::new(Box::new(MockBankAccountServices::default()));
        AccountTestFramework::with(services)
            .given(vec![previous])
            .when(BankAccountCommand::WithdrawMoney {
                amount: Money::new(dec!(100.00), Currency::USD),
            })
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_withdraw_money_funds_not_available() {
        let services = BankAccountServices::new(Box::new(MockBankAccountServices::default()));
        AccountTestFramework::with(services)
            .given_no_previous_events()
            .when(BankAccountCommand::WithdrawMoney {
                amount: Money::new(dec!(200.00), Currency::USD),
            })
            .then_expect_error_message("insufficient funds");
    }

    pub struct MockBankAccountServices {
        atm_withdrawal_response: Mutex<Option<Result<(), AtmError>>>,
        validate_check_response: Mutex<Option<Result<(), CheckingError>>>,
    }

    impl Default for MockBankAccountServices {
        fn default() -> Self {
            Self {
                atm_withdrawal_response: Mutex::new(None),
                validate_check_response: Mutex::new(None),
            }
        }
    }

    impl MockBankAccountServices {
        fn set_atm_withdrawal_response(&self, response: Result<(), AtmError>) {
            *self.atm_withdrawal_response.lock().unwrap() = Some(response);
        }
        fn set_validate_check_response(&self, response: Result<(), CheckingError>) {
            *self.validate_check_response.lock().unwrap() = Some(response);
        }
    }

    #[async_trait]
    impl BankAccountApi for MockBankAccountServices {
        async fn atm_withdrawal(&self, _atm_id: &str, _amount: f64) -> Result<(), AtmError> {
            self.atm_withdrawal_response.lock().unwrap().take().unwrap()
        }

        async fn validate_check(
            &self,
            _account_id: &str,
            _check_number: &str,
        ) -> Result<(), CheckingError> {
            self.validate_check_response.lock().unwrap().take().unwrap()
        }
    }
}
