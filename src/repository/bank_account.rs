use async_trait::async_trait;
use commands::BankAccountCommand;
use cqrs_es::mem_store::MemStore;
use cqrs_es::{Aggregate, CqrsFramework, EventEnvelope, Query};
use events::BankAccountEvent;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

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

struct SimpleLoggingQuery {}

#[async_trait]
impl Query<BankAccount> for SimpleLoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<BankAccount>]) {
        for event in events {
            println!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
        }
    }
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;
    use cqrs_es::test::TestFramework;
    use events::BankAccountEvent;
    use rust_decimal_macros::dec;

    type AccountTestFramework = TestFramework<BankAccount>;

    #[test]
    fn test_deposit_money() {
        let expected = BankAccountEvent::CustomerDepositedMoney {
            amount: Money::new(dec!(200.00), Currency::USD),
            balance: Money::new(dec!(200.00), Currency::USD),
        };

        AccountTestFramework::with(BankAccountServices)
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

        AccountTestFramework::with(BankAccountServices)
            .given(vec![previous])
            .when(BankAccountCommand::WithdrawMoney {
                amount: Money::new(dec!(100.00), Currency::USD),
            })
            .then_expect_events(vec![expected]);
    }

    #[test]
    fn test_withdraw_money_funds_not_available() {
        AccountTestFramework::with(BankAccountServices)
            .given_no_previous_events()
            .when(BankAccountCommand::WithdrawMoney {
                amount: Money::new(dec!(200.00), Currency::USD),
            })
            .then_expect_error_message("insufficient funds");
    }
}

#[tokio::test]
async fn test_event_store() {
    let event_store = MemStore::<BankAccount>::default();
    let query = SimpleLoggingQuery {};
    let cqrs = CqrsFramework::new(event_store, vec![Box::new(query)], BankAccountServices);

    let aggregate_id = "aggregate-instance-A";

    // deposit $1000
    cqrs.execute(
        aggregate_id,
        BankAccountCommand::DepositMoney {
            amount: Money::new(dec!(1000.00), Currency::USD),
        },
    )
    .await
    .unwrap();

    // write a check for $236.15
    cqrs.execute(
        aggregate_id,
        BankAccountCommand::WriteCheck {
            check_number: "1337".to_string(),
            amount: Money::new(dec!(236.15), Currency::USD),
        },
    )
    .await
    .unwrap();
}
