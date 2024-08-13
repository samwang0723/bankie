use async_trait::async_trait;
use command::{BankAccountCommand, LedgerCommand};
use cqrs_es::Aggregate;
use event::{BaseEvent, Event};
use finance::{JournalEntry, JournalLine, Transaction};
use models::LedgerAction;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::service::{BankAccountServices, MockLedgerServices};
use crate::{common, event_sourcing::*};

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
            BankAccountCommand::OpenAccount {
                id,
                account_type,
                kind,
                user_id,
                currency,
            } => {
                // Validate if can open bank account
                match services
                    .services
                    .validate_account_creation(id, user_id.clone(), currency, kind)
                    .await
                {
                    Ok(valid) => {
                        if !valid {
                            return Err("validation failed".into());
                        }
                    }
                    Err(_) => return Err("validation failed".into()),
                };

                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::BankAccountEvent::AccountOpened {
                    base_event,
                    account_type,
                    kind,
                    user_id,
                    currency,
                }])
            }
            BankAccountCommand::ApproveAccount { id, ledger_id } => {
                // Initialize ledger while account being approved
                let command = LedgerCommand::Init {
                    id: ledger_id,
                    account_id: id,
                };
                if services
                    .services
                    .note_ledger(ledger_id.to_string(), command)
                    .await
                    .is_err()
                {
                    return Err("ledger write failed".into());
                };

                // Update bank account to be approved state
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::BankAccountEvent::AccountKycApproved {
                    ledger_id: ledger_id.to_string(),
                    base_event,
                }])
            }
            BankAccountCommand::Deposit { amount } => {
                let house_account_ledger =
                    match services.services.get_house_account(amount.currency).await {
                        Ok(id) => id,
                        Err(_) => return Err("house account not found".into()),
                    };

                // Validate if ledger can do action
                match services
                    .services
                    .validate(
                        Uuid::parse_str(&self.id).unwrap(),
                        LedgerAction::Deposit,
                        amount,
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(_) => return Err("validation failed".into()),
                };

                // Create transaction and journal for accounting purpose
                let transaction = Transaction {
                    id: Uuid::new_v4(),
                    bank_account_id: Uuid::parse_str(&self.id).unwrap(),
                    transaction_reference: common::snowflake::generate_transaction_reference("DE"),
                    transaction_date: chrono::Utc::now().date_naive(),
                    amount: amount.amount,
                    currency: amount.currency.to_string(),
                    description: None,
                    journal_entry_id: None,
                    status: "processing".to_string(),
                };
                let journal_entry = JournalEntry {
                    id: Uuid::new_v4(),
                    entry_date: chrono::Utc::now().date_naive(),
                    description: None,
                    status: "posted".to_string(),
                };
                let journal_lines = vec![
                    JournalLine {
                        id: Uuid::new_v4(),
                        journal_entry_id: None,
                        ledger_id: house_account_ledger,
                        credit_amount: Decimal::ZERO,
                        debit_amount: amount.amount,
                        currency: amount.currency.to_string(),
                        description: None,
                    },
                    JournalLine {
                        id: Uuid::new_v4(),
                        journal_entry_id: None,
                        ledger_id: self.ledger_id.clone(),
                        debit_amount: Decimal::ZERO,
                        credit_amount: amount.amount,
                        currency: amount.currency.to_string(),
                        description: None,
                    },
                ];
                match services
                    .services
                    .create_transaction_with_journal(transaction, journal_entry, journal_lines)
                    .await
                {
                    Ok(transaction_id) => {
                        // Once get the created transaction, perform ledger note and update
                        // balance accordingly.
                        let command = LedgerCommand::Credit {
                            id: Uuid::parse_str(&self.ledger_id).unwrap(),
                            account_id: Uuid::parse_str(&self.id).unwrap(),
                            transaction_id,
                            amount,
                        };
                        if services
                            .services
                            .note_ledger(self.ledger_id.clone(), command)
                            .await
                            .is_err()
                        {
                            return Err("ledger write failed".into());
                        };
                        if (services.services.complete_transaction(transaction_id).await).is_err() {
                            return Err("transaction complete failed".into());
                        }

                        Ok(vec![])
                    }
                    Err(_) => Err("transaction write failed".into()),
                }
            }
            BankAccountCommand::Withdrawl { amount } => {
                let house_account_ledger =
                    match services.services.get_house_account(amount.currency).await {
                        Ok(id) => id,
                        Err(_) => return Err("house account not found".into()),
                    };

                match services
                    .services
                    .validate(
                        Uuid::parse_str(&self.id).unwrap(),
                        LedgerAction::Withdraw,
                        amount,
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(_) => return Err("validation failed".into()),
                };

                let transaction = Transaction {
                    id: Uuid::new_v4(),
                    bank_account_id: Uuid::parse_str(&self.id).unwrap(),
                    transaction_reference: common::snowflake::generate_transaction_reference("WI"),
                    transaction_date: chrono::Utc::now().date_naive(),
                    amount: amount.amount,
                    currency: amount.currency.to_string(),
                    description: None,
                    journal_entry_id: None,
                    status: "processing".to_string(),
                };
                let journal_entry = JournalEntry {
                    id: Uuid::new_v4(),
                    entry_date: chrono::Utc::now().date_naive(),
                    description: None,
                    status: "posted".to_string(),
                };
                let journal_lines = vec![
                    JournalLine {
                        id: Uuid::new_v4(),
                        journal_entry_id: None,
                        ledger_id: house_account_ledger,
                        debit_amount: Decimal::ZERO,
                        credit_amount: amount.amount,
                        currency: amount.currency.to_string(),
                        description: None,
                    },
                    JournalLine {
                        id: Uuid::new_v4(),
                        journal_entry_id: None,
                        ledger_id: self.ledger_id.clone(),
                        credit_amount: Decimal::ZERO,
                        debit_amount: amount.amount,
                        currency: amount.currency.to_string(),
                        description: None,
                    },
                ];
                match services
                    .services
                    .create_transaction_with_journal(transaction, journal_entry, journal_lines)
                    .await
                {
                    Ok(transaction_id) => {
                        let command = LedgerCommand::Debit {
                            id: Uuid::parse_str(&self.ledger_id).unwrap(),
                            account_id: Uuid::parse_str(&self.id).unwrap(),
                            transaction_id,
                            amount,
                        };
                        if services
                            .services
                            .note_ledger(self.ledger_id.clone(), command)
                            .await
                            .is_err()
                        {
                            return Err("ledger write failed".into());
                        };

                        if (services.services.complete_transaction(transaction_id).await).is_err() {
                            return Err("transaction complete failed".into());
                        }

                        Ok(vec![])
                    }
                    Err(_) => Err("transaction write failed".into()),
                }
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::BankAccountEvent::AccountOpened {
                base_event,
                account_type,
                kind,
                user_id,
                currency,
            } => {
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Pending;
                self.timestamp = base_event.get_created_at();
                self.account_type = account_type;
                self.kind = kind;
                self.currency = currency;
                self.user_id = user_id;
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
impl Aggregate for models::Ledger {
    type Command = command::LedgerCommand;
    type Event = events::LedgerEvent;
    type Error = error::LedgerError;
    type Services = MockLedgerServices;

    // This identifier should be unique to the system.
    fn aggregate_type() -> String {
        "ledger".to_string()
    }

    // The aggregate logic goes here. Note that this will be the _bulk_ of a CQRS system
    // so expect to use helper functions elsewhere to keep the code clean.
    async fn handle(
        &self,
        command: Self::Command,
        _services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            LedgerCommand::Init { id, account_id } => {
                let mut base_event = BaseEvent::default();
                base_event.set_aggregate_id(id);
                base_event.set_parent_id(account_id);
                base_event.set_created_at(chrono::Utc::now());
                Ok(vec![events::LedgerEvent::LedgerInitiated {
                    base_event: base_event.clone(),
                }])
            }
            LedgerCommand::Debit {
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
                        transaction_type: "debit_hold".to_string(),
                        available_delta: Money::new(Decimal::ZERO - amount.amount, Currency::USD),
                        pending_delta: Money::new(amount.amount, Currency::USD),
                        base_event: base_event.clone(),
                    },
                    events::LedgerEvent::LedgerUpdated {
                        amount,
                        transaction_id: transaction_id.to_string(),
                        transaction_type: "debit_release".to_string(),
                        available_delta: Money::new(Decimal::ZERO, Currency::USD),
                        pending_delta: Money::new(Decimal::ZERO - amount.amount, Currency::USD),
                        base_event,
                    },
                ])
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
                        available_delta: Money::new(Decimal::ZERO, Currency::USD),
                        pending_delta: Money::new(amount.amount, Currency::USD),
                        base_event: base_event.clone(),
                    },
                    events::LedgerEvent::LedgerUpdated {
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
            events::LedgerEvent::LedgerInitiated { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.account_id = base_event.get_parent_id();
                self.amount = Money::new(Decimal::ZERO, Currency::USD);
                self.available = Money::new(Decimal::ZERO, Currency::USD);
                self.pending = Money::new(Decimal::ZERO, Currency::USD);
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

// The aggregate tests are the most important part of a CQRS system.
// The simplicity and flexibility of these tests are a good part of what
// makes an event sourced system so friendly to changing business requirements.
#[cfg(test)]
mod aggregate_tests {
    use async_trait::async_trait;
    use lazy_static::lazy_static;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::sync::Mutex;
    use uuid::Uuid;

    use cqrs_es::test::TestFramework;

    use crate::{
        common::money::{Currency, Money},
        service::{BankAccountApi, BankAccountServices, MockLedgerServices},
    };

    use super::{
        command::{BankAccountCommand, LedgerCommand},
        event::{BaseEvent, Event},
        events::{BankAccountEvent, LedgerEvent},
        finance::{JournalEntry, JournalLine, Transaction},
        models::{BankAccount, BankAccountKind, BankAccountType, Ledger, LedgerAction},
    };

    // A test framework that will apply our events and command
    // and verify that the logic works as expected.
    type AccountTestFramework = TestFramework<BankAccount>;
    type LedgerTestFramework = TestFramework<Ledger>;

    lazy_static! {
        static ref LEDGER_ID: Uuid = Uuid::new_v4();
        static ref ACCOUNT_ID: Uuid = Uuid::new_v4();
        static ref TRANSACTION_ID: Uuid = Uuid::new_v4();
    }

    fn create_base_event(uuid: Uuid) -> BaseEvent {
        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_created_at(chrono::Utc::now());
        base_event
    }

    fn create_ledger_base_event(uuid: Uuid, parent_id: Uuid) -> BaseEvent {
        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_parent_id(parent_id);
        base_event.set_created_at(chrono::Utc::now());
        base_event
    }

    fn setup_mock_services() -> MockBankAccountServices {
        let mock_services = MockBankAccountServices::default();
        mock_services.set_write_ledger_response(Ok(()));
        mock_services.set_write_transaction_response(Ok(Uuid::new_v4()));
        mock_services.set_validate_response(Ok(()));
        mock_services
    }

    macro_rules! test_case {
        ($name:ident, $given:expr, $command:expr, $expected:expr) => {
            #[test]
            fn $name() {
                let services = BankAccountServices::new(Box::new(setup_mock_services()));
                AccountTestFramework::with(services)
                    .given($given)
                    .when($command)
                    .then_expect_events($expected);
            }
        };
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

    test_case!(
        test_account_creation,
        vec![],
        BankAccountCommand::OpenAccount {
            id: *ACCOUNT_ID,
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            user_id: "user".to_string(),
            currency: Currency::USD
        },
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            user_id: "user".to_string(),
            currency: Currency::USD
        }]
    );

    test_case!(
        test_account_kyc_approved,
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            user_id: "user".to_string(),
            currency: Currency::USD
        }],
        BankAccountCommand::ApproveAccount {
            id: *ACCOUNT_ID,
            ledger_id: *LEDGER_ID
        },
        vec![BankAccountEvent::AccountKycApproved {
            ledger_id: LEDGER_ID.to_string(),
            base_event: create_base_event(*ACCOUNT_ID)
        }]
    );

    ledger_test_case!(
        test_ledger_init,
        vec![],
        LedgerCommand::Init {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID
        },
        vec![LedgerEvent::LedgerInitiated {
            base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
        }]
    );

    test_case!(
        test_deposit,
        vec![
            BankAccountEvent::AccountOpened {
                base_event: create_base_event(*ACCOUNT_ID),
                account_type: BankAccountType::Retail,
                kind: BankAccountKind::Checking,
                user_id: "user".to_string(),
                currency: Currency::USD
            },
            BankAccountEvent::AccountKycApproved {
                ledger_id: LEDGER_ID.to_string(),
                base_event: create_base_event(*ACCOUNT_ID)
            }
        ],
        BankAccountCommand::Deposit {
            amount: Money::new(dec!(1000.0), Currency::USD)
        },
        vec![]
    );

    ledger_test_case!(
        test_ledger_deposit,
        vec![LedgerEvent::LedgerInitiated {
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

    test_case!(
        test_withdrawl,
        vec![
            BankAccountEvent::AccountOpened {
                base_event: create_base_event(*ACCOUNT_ID),
                account_type: BankAccountType::Retail,
                kind: BankAccountKind::Checking,
                user_id: "user".to_string(),
                currency: Currency::USD
            },
            BankAccountEvent::AccountKycApproved {
                ledger_id: LEDGER_ID.to_string(),
                base_event: create_base_event(*ACCOUNT_ID)
            }
        ],
        BankAccountCommand::Withdrawl {
            amount: Money::new(dec!(500.0), Currency::USD)
        },
        vec![]
    );

    ledger_test_case!(
        test_ledger_withdrawl,
        vec![
            LedgerEvent::LedgerInitiated {
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
        LedgerCommand::Debit {
            id: *LEDGER_ID,
            account_id: *ACCOUNT_ID,
            transaction_id: *TRANSACTION_ID,
            amount: Money::new(dec!(200.0), Currency::USD),
        },
        vec![
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(200.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "debit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO - dec!(200.0), Currency::USD),
                pending_delta: Money::new(dec!(200.0), Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            },
            LedgerEvent::LedgerUpdated {
                amount: Money::new(dec!(200.0), Currency::USD),
                transaction_id: TRANSACTION_ID.to_string(),
                transaction_type: "debit_release".to_string(),
                pending_delta: Money::new(Decimal::ZERO - dec!(200.0), Currency::USD),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                base_event: create_ledger_base_event(*LEDGER_ID, *ACCOUNT_ID)
            }
        ]
    );

    pub struct MockBankAccountServices {
        write_ledger_response: Mutex<Option<Result<(), anyhow::Error>>>,
        write_transaction_response: Mutex<Option<Result<Uuid, anyhow::Error>>>,
        validate_response: Mutex<Option<Result<(), anyhow::Error>>>,
    }

    impl Default for MockBankAccountServices {
        fn default() -> Self {
            Self {
                write_ledger_response: Mutex::new(None),
                write_transaction_response: Mutex::new(None),
                validate_response: Mutex::new(None),
            }
        }
    }

    impl MockBankAccountServices {
        fn set_write_ledger_response(&self, response: Result<(), anyhow::Error>) {
            *self.write_ledger_response.lock().unwrap() = Some(response);
        }

        fn set_write_transaction_response(&self, response: Result<Uuid, anyhow::Error>) {
            *self.write_transaction_response.lock().unwrap() = Some(response);
        }

        fn set_validate_response(&self, response: Result<(), anyhow::Error>) {
            *self.validate_response.lock().unwrap() = Some(response);
        }
    }

    #[async_trait]
    impl BankAccountApi for MockBankAccountServices {
        async fn note_ledger(
            &self,
            _ledger_id: String,
            _command: LedgerCommand,
        ) -> Result<(), anyhow::Error> {
            self.write_ledger_response.lock().unwrap().take().unwrap()
        }

        async fn complete_transaction(&self, _transaction_id: Uuid) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn create_transaction_with_journal(
            &self,
            _transaction: Transaction,
            _journal_entry: JournalEntry,
            _journal_lines: Vec<JournalLine>,
        ) -> Result<Uuid, anyhow::Error> {
            self.write_transaction_response
                .lock()
                .unwrap()
                .take()
                .unwrap()
        }

        async fn validate(
            &self,
            _account_id: Uuid,
            _action: LedgerAction,
            _amount: Money,
        ) -> Result<(), anyhow::Error> {
            self.validate_response.lock().unwrap().take().unwrap()
        }

        async fn get_house_account(&self, _currency: Currency) -> Result<String, anyhow::Error> {
            Ok(Uuid::new_v4().to_string())
        }

        async fn validate_account_creation(
            &self,
            _account_id: Uuid,
            _user_id: String,
            _currency: Currency,
            _kind: BankAccountKind,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }
    }
}
