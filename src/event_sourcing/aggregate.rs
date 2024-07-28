use async_trait::async_trait;
use command::BankAccountCommand;
use cqrs_es::Aggregate;
use event::BankAccountEvent;
use rust_decimal::Decimal;

use crate::common::money::{Currency, Money};
use crate::domain::models::BankAccount;
use crate::event_sourcing::*;
use crate::service::BankAccountServices;

#[async_trait]
impl Aggregate for BankAccount {
    type Command = command::BankAccountCommand;
    type Event = event::BankAccountEvent;
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
                Ok(vec![BankAccountEvent::AccountOpened { account_id }])
            }
            BankAccountCommand::DepositMoney { amount } => {
                let balance = self.balance + amount;
                Ok(vec![BankAccountEvent::CustomerDepositedMoney {
                    amount,
                    balance,
                }])
            }
            BankAccountCommand::WithdrawMoney { amount, atm_id } => {
                let balance = self.balance - amount;
                if balance < Money::new(Decimal::ZERO, Currency::USD) {
                    return Err("funds not available".into());
                }
                if services
                    .services
                    .atm_withdrawal(&atm_id, amount)
                    .await
                    .is_err()
                {
                    return Err("atm rule violation".into());
                };
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
                if services
                    .services
                    .validate_check(&self.account_id, &check_number)
                    .await
                    .is_err()
                {
                    return Err("check invalid".into());
                };
                Ok(vec![BankAccountEvent::CustomerWroteCheck {
                    check_number,
                    amount,
                    balance,
                }])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            event::BankAccountEvent::AccountOpened { account_id } => {
                self.account_id = account_id;
                self.balance = Money::new(Decimal::ZERO, Currency::USD);
            }
            event::BankAccountEvent::CustomerDepositedMoney { amount: _, balance } => {
                self.balance = balance;
            }
            event::BankAccountEvent::CustomerWithdrewCash { amount: _, balance } => {
                self.balance = balance;
            }
            event::BankAccountEvent::CustomerWroteCheck {
                check_number: _,
                amount: _,
                balance,
            } => {
                self.balance = balance;
            }
        }
    }
}

#[cfg(test)]
mod aggregate_tests {
    use crate::{
        common::money::{Currency, Money},
        event_sourcing::command::BankAccountCommand,
        service::{AtmError, BankAccountApi, BankAccountServices, CheckingError},
    };

    use super::*;
    use cqrs_es::test::TestFramework;
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
                atm_id: "1234".to_string(),
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
                atm_id: "1234".to_string(),
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

    #[allow(dead_code)]
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
        async fn atm_withdrawal(&self, _atm_id: &str, _amount: Money) -> Result<(), AtmError> {
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
