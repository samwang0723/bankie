use async_trait::async_trait;
use command::{BankAccountCommand, LedgerCommand};
use cqrs_es::Aggregate;
use event::Event;
use models::LedgerAction;
use uuid::Uuid;

use crate::domain::*;
use crate::event_sourcing::*;
use crate::service::BankAccountServices;

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
                helper::validate_account_creation(services, id, user_id.clone(), currency, kind)
                    .await?;

                Ok(vec![events::BankAccountEvent::AccountOpened {
                    base_event: helper::create_base_event(id),
                    account_type,
                    kind,
                    user_id,
                    currency,
                }])
            }
            BankAccountCommand::ApproveAccount { id, ledger_id } => {
                let bank_account = services
                    .services
                    .get_bank_account(id)
                    .await
                    .map_err(|_| "account not found")?;

                helper::init_ledger(services, ledger_id, id, bank_account.currency).await?;

                Ok(vec![events::BankAccountEvent::AccountKycApproved {
                    ledger_id: ledger_id.to_string(),
                    base_event: helper::create_base_event(id),
                }])
            }
            BankAccountCommand::Deposit { id: _, amount } => {
                let house_account = services
                    .services
                    .get_house_account(amount.currency)
                    .await
                    .map_err(|_| "house account not found")?;

                helper::validate_ledger_action(self, services, LedgerAction::Deposit, amount)
                    .await?;

                let transaction_id = helper::create_transaction_with_journal(
                    self,
                    services,
                    amount,
                    house_account.ledger_id,
                    LedgerAction::Deposit,
                )
                .await?;

                helper::note_ledger_and_complete_transaction(
                    self,
                    services,
                    transaction_id,
                    LedgerCommand::Credit {
                        id: Uuid::parse_str(&self.ledger_id).unwrap(),
                        account_id: Uuid::parse_str(&self.id).unwrap(),
                        transaction_id,
                        amount,
                    },
                )
                .await
            }
            BankAccountCommand::Withdrawal { id: _, amount } => {
                let house_account = services
                    .services
                    .get_house_account(amount.currency)
                    .await
                    .map_err(|_| "house account not found")?;

                helper::validate_ledger_action(self, services, LedgerAction::Withdraw, amount)
                    .await?;

                let transaction_id = helper::create_transaction_with_journal(
                    self,
                    services,
                    amount,
                    house_account.ledger_id,
                    LedgerAction::Withdraw,
                )
                .await?;

                helper::note_ledger_and_complete_transaction(
                    self,
                    services,
                    transaction_id,
                    LedgerCommand::Debit {
                        id: Uuid::parse_str(&self.ledger_id).unwrap(),
                        account_id: Uuid::parse_str(&self.id).unwrap(),
                        transaction_id,
                        amount,
                    },
                )
                .await
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

// The aggregate tests are the most important part of a CQRS system.
// The simplicity and flexibility of these tests are a good part of what
// makes an event sourced system so friendly to changing business requirements.
#[cfg(test)]
mod aggregate_tests {
    use async_trait::async_trait;
    use lazy_static::lazy_static;
    use rust_decimal_macros::dec;
    use std::sync::Mutex;
    use uuid::Uuid;

    use cqrs_es::test::TestFramework;

    use crate::{
        common::money::{Currency, Money},
        service::{BankAccountApi, BankAccountServices},
    };

    use super::{
        command::{BankAccountCommand, LedgerCommand},
        event::{BaseEvent, Event},
        events::BankAccountEvent,
        finance::{JournalEntry, JournalLine, Transaction},
        models::{
            BankAccount, BankAccountKind, BankAccountType, BankAccountView, HouseAccount,
            LedgerAction,
        },
    };

    // A test framework that will apply our events and command
    // and verify that the logic works as expected.
    type AccountTestFramework = TestFramework<BankAccount>;

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
            id: *ACCOUNT_ID,
            amount: Money::new(dec!(1000.0), Currency::USD)
        },
        vec![]
    );

    test_case!(
        test_withdrawal,
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
        BankAccountCommand::Withdrawal {
            id: *ACCOUNT_ID,
            amount: Money::new(dec!(500.0), Currency::USD)
        },
        vec![]
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

        async fn fail_transaction(&self, _transaction_id: Uuid) -> Result<(), anyhow::Error> {
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

        async fn get_house_account(
            &self,
            _currency: Currency,
        ) -> Result<HouseAccount, anyhow::Error> {
            Ok(HouseAccount::default())
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

        async fn get_bank_account(
            &self,
            _account_id: Uuid,
        ) -> Result<BankAccountView, anyhow::Error> {
            Ok(BankAccountView::default())
        }
    }
}
