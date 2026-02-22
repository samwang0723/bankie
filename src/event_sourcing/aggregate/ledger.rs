use async_trait::async_trait;
use command::LedgerCommand;
use cqrs_es::Aggregate;
use event::{BaseEvent, Event};
use rust_decimal::Decimal;

use crate::common::money::Money;
use crate::domain::*;
use crate::event_sourcing::*;
use crate::service::MockLedgerServices;

#[async_trait]
impl Aggregate for models::Ledger {
    type Command = command::LedgerCommand;
    type Event = events::LedgerEvent;
    type Error = error::LedgerError;
    type Services = MockLedgerServices;

    fn aggregate_type() -> String {
        "ledger".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            LedgerCommand::Init {
                id,
                account_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerInitiated {
                    amount,
                    base_event: base_event.clone(),
                }])
            }
            LedgerCommand::DebitHold {
                id,
                account_id,
                transaction_id,
                amount,
            } => {
                // C1 FIX: Check balance sufficiency using aggregate state (strongly consistent),
                // not the eventually-consistent view. This prevents the TOCTOU race condition.
                if self.available < amount {
                    return Err("Insufficient funds".into());
                }

                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerUpdated {
                    amount,
                    transaction_id: transaction_id.to_string(),
                    transaction_type: "debit_hold".to_string(),
                    available_delta: Money::new(Decimal::ZERO - amount.amount, amount.currency),
                    pending_delta: Money::new(amount.amount, amount.currency),
                    base_event: base_event.clone(),
                }])
            }
            LedgerCommand::DebitRelease {
                id,
                account_id,
                transaction_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerUpdated {
                    amount,
                    transaction_id: transaction_id.to_string(),
                    transaction_type: "debit_release".to_string(),
                    available_delta: Money::new(Decimal::ZERO, amount.currency),
                    pending_delta: Money::new(Decimal::ZERO - amount.amount, amount.currency),
                    base_event,
                }])
            }
            LedgerCommand::Credit {
                id,
                account_id,
                transaction_id,
                amount,
            } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![
                    events::LedgerEvent::LedgerUpdated {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "credit_hold".to_string(),
                        available_delta: Money::new(Decimal::ZERO, amount.currency),
                        pending_delta: Money::new(amount.amount, amount.currency),
                        base_event: base_event.clone(),
                    },
                    events::LedgerEvent::LedgerUpdated {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "credit_release".to_string(),
                        available_delta: Money::new(amount.amount, amount.currency),
                        pending_delta: Money::new(Decimal::ZERO - amount.amount, amount.currency),
                        base_event,
                    },
                ])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::LedgerEvent::LedgerInitiated { base_event, amount } => {
                self.id = base_event.get_aggregate_id();
                self.account_id = base_event.get_parent_id();
                self.amount = amount;
                self.available = amount;
                self.pending = Money::new(Decimal::ZERO, amount.currency);
                self.timestamp = base_event.get_created_at();
            }
            events::LedgerEvent::LedgerUpdated {
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

#[cfg(test)]
mod aggregate_tests {
    use lazy_static::lazy_static;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    use cqrs_es::test::TestFramework;

    use crate::{
        common::money::{Currency, Money},
        service::MockLedgerServices,
    };

    use super::{
        command::LedgerCommand,
        event::{BaseEvent, Event},
        events::LedgerEvent,
        models::Ledger,
    };

    type LedgerTestFramework = TestFramework<Ledger>;

    lazy_static! {
        static ref LEDGER_ID: Uuid = Uuid::new_v4();
        static ref ACCOUNT_ID: Uuid = Uuid::new_v4();
        static ref TRANSACTION_ID: Uuid = Uuid::new_v4();
    }

    fn create_ledger_base_event(uuid: Uuid, parent_id: Uuid) -> BaseEvent {
        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_parent_id(parent_id);
        base_event.set_created_at(chrono::Utc::now());
        base_event
    }

    macro_rules! ledger_test_case {
        ($name:ident, $given:expr, $command:expr, $expected:expr) => {
            #[test]
            fn $name() {
                LedgerTestFramework::with(MockLedgerServices {})
                    .given($given)
                    .when($command)
                    .then_expect_events($expected);
            }
        };
    }

    ledger_test_case!(
        test_ledger_init,
        vec![],
        LedgerCommand::Init {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            amount: Money::new(dec!(1000.0), Currency::USD),
        },
        vec![LedgerEvent::LedgerInitiated {
            amount: Money::new(dec!(1000.0), Currency::USD),
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
        }]
    );

    ledger_test_case!(
        test_ledger_deposit,
        vec![LedgerEvent::LedgerInitiated {
            amount: Money::new(dec!(1000.0), Currency::USD),
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
        }],
        LedgerCommand::Credit {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(1000.0), Currency::USD),
        },
        vec![
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                pending_delta: Money::new(Decimal::ZERO - dec!(1000.0), Currency::USD),
                available_delta: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            }
        ]
    );

    ledger_test_case!(
        test_ledger_withdrawal,
        vec![
            LedgerEvent::LedgerInitiated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                pending_delta: Money::new(Decimal::ZERO - dec!(1000.0), Currency::USD),
                available_delta: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            }
        ],
        LedgerCommand::DebitHold {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(200.0), Currency::USD),
        },
        vec![LedgerEvent::LedgerUpdated {
            amount: Money::new(dec!(200.0), Currency::USD),
            transaction_id: TRANSACTION_ID.to_string(),
            transaction_type: "debit_hold".to_string(),
            available_delta: Money::new(Decimal::ZERO - dec!(200.0), Currency::USD),
            pending_delta: Money::new(dec!(200.0), Currency::USD),
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
        }]
    );

    #[test]
    fn test_debit_hold_insufficient_funds() {
        // C1 test: DebitHold on aggregate with insufficient funds should fail
        LedgerTestFramework::with(MockLedgerServices {})
            .given(vec![LedgerEvent::LedgerInitiated {
                amount: Money::new(dec!(100.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            }])
            .when(LedgerCommand::DebitHold {
                id: *LEDGER_ID,
                account_id: *ACCOUNT_ID,
                transaction_id: *TRANSACTION_ID,
                amount: Money::new(dec!(200.0), Currency::USD),
            })
            .then_expect_error_message("Insufficient funds");
    }

    // L1: DebitRelease — verify pending decreases after hold
    fn funded_ledger_events() -> Vec<LedgerEvent> {
        vec![
            LedgerEvent::LedgerInitiated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            },
            // Credit: hold + release (net effect: available += 1000)
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(1000.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                available_delta: Money::new(dec!(1000.0), Currency::USD),
                pending_delta: Money::new(Decimal::ZERO - dec!(1000.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            },
        ]
    }

    ledger_test_case!(
        test_debit_release,
        {
            let mut events = funded_ledger_events();
            // DebitHold $200: available -= 200, pending += 200
            events.push(LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(200.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "debit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO - dec!(200.0), Currency::USD),
                pending_delta: Money::new(dec!(200.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            });
            events
        },
        LedgerCommand::DebitRelease {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(200.0), Currency::USD),
        },
        vec![LedgerEvent::LedgerUpdated {
            amount: Money::new(dec!(200.0), Currency::USD),
            transaction_id: TRANSACTION_ID.to_string(),
            transaction_type: "debit_release".to_string(),
            available_delta: Money::new(Decimal::ZERO, Currency::USD),
            pending_delta: Money::new(Decimal::ZERO - dec!(200.0), Currency::USD),
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
        }]
    );

    // L3: DebitHold for exact available amount (boundary)
    ledger_test_case!(
        test_debit_hold_exact_balance,
        funded_ledger_events(),
        LedgerCommand::DebitHold {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(2000.0), Currency::USD),
        },
        vec![LedgerEvent::LedgerUpdated {
            amount: Money::new(dec!(2000.0), Currency::USD),
            transaction_id: TRANSACTION_ID.to_string(),
            transaction_type: "debit_hold".to_string(),
            available_delta: Money::new(Decimal::ZERO - dec!(2000.0), Currency::USD),
            pending_delta: Money::new(dec!(2000.0), Currency::USD),
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
        }]
    );

    // L4: Credit on uninitialized ledger (available = 0, no init event)
    ledger_test_case!(
        test_credit_on_uninitialized_ledger,
        vec![],
        LedgerCommand::Credit {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(500.0), Currency::USD),
        },
        vec![
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(500.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(500.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(500.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                available_delta: Money::new(dec!(500.0), Currency::USD),
                pending_delta: Money::new(Decimal::ZERO - dec!(500.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            }
        ]
    );

    // L5: Multiple sequential credits verify cumulative balance
    ledger_test_case!(
        test_multiple_credits,
        {
            let mut events = funded_ledger_events();
            // Add a second credit of $500
            events.push(LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(500.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(500.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            });
            events.push(LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(500.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                available_delta: Money::new(dec!(500.0), Currency::USD),
                pending_delta: Money::new(Decimal::ZERO - dec!(500.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            });
            events
        },
        // Third credit of $300 — aggregate should have available=2500 at this point
        LedgerCommand::Credit {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(300.0), Currency::USD),
        },
        vec![
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(300.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(dec!(300.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(300.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "credit_release".to_string(),
                available_delta: Money::new(dec!(300.0), Currency::USD),
                pending_delta: Money::new(Decimal::ZERO - dec!(300.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID),
            }
        ]
    );
}
