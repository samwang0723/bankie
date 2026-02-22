use async_trait::async_trait;
use command::BankAccountCommand;
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

    fn aggregate_type() -> String {
        "bank_account".to_string()
    }

    async fn handle(
        &self,
        command: Self::Command,
        services: &Self::Services,
    ) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            BankAccountCommand::OpenAccount {
                id,
                parent_id: _,
                account_number,
                account_type,
                kind,
                external_reference_id,
                currency,
            } => {
                helper::validate_account_creation(
                    services,
                    id,
                    external_reference_id.clone(),
                    currency,
                    kind,
                )
                .await?;

                // Resolve parent_id for sub-accounts (Interest/Yield)
                let resolved_parent_id = if kind != models::BankAccountKind::Checking {
                    services
                        .services
                        .find_checking_account(external_reference_id.clone(), currency)
                        .await
                        .ok()
                        .flatten()
                } else {
                    None
                };

                let mut base_event = helper::create_base_event(id);
                if let Some(parent_uuid) = resolved_parent_id {
                    base_event.set_parent_id(parent_uuid);
                }

                Ok(vec![events::BankAccountEvent::AccountOpened {
                    base_event,
                    account_type,
                    kind,
                    external_reference_id,
                    account_number,
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
            BankAccountCommand::FreezeAccount { id } => {
                if self.status != models::BankAccountStatus::Approved {
                    return Err("account must be Approved to freeze".into());
                }
                Ok(vec![events::BankAccountEvent::AccountFrozen {
                    base_event: helper::create_base_event(id),
                }])
            }
            BankAccountCommand::UnfreezeAccount { id } => {
                if self.status != models::BankAccountStatus::Freeze {
                    return Err("account must be Frozen to unfreeze".into());
                }
                Ok(vec![events::BankAccountEvent::AccountUnfrozen {
                    base_event: helper::create_base_event(id),
                }])
            }
            BankAccountCommand::CloseAccount { id } => {
                if self.status != models::BankAccountStatus::Approved {
                    return Err("account must be Approved to close".into());
                }
                // Verify ledger balance is zero
                let (available, pending) = services
                    .services
                    .get_ledger_balance(id)
                    .await
                    .map_err(|_| "failed to check ledger balance")?;
                if available.amount != rust_decimal::Decimal::ZERO
                    || pending.amount != rust_decimal::Decimal::ZERO
                {
                    return Err("account balance must be zero to close".into());
                }
                Ok(vec![events::BankAccountEvent::AccountClosed {
                    base_event: helper::create_base_event(id),
                }])
            }
            BankAccountCommand::Deposit { id: _, amount } => {
                let asset_code = amount.asset_code();
                let house_account = services
                    .services
                    .get_house_account(&asset_code)
                    .await
                    .map_err(|_| "house account not found")?;

                helper::create_transaction_with_journal(
                    self,
                    services,
                    amount,
                    house_account.ledger_id,
                    LedgerAction::Deposit,
                )
                .await?;

                Ok(vec![])
            }
            BankAccountCommand::Withdrawal { id, amount } => {
                let asset_code = amount.asset_code();
                let house_account = services
                    .services
                    .get_house_account(&asset_code)
                    .await
                    .map_err(|_| "house account not found")?;

                let transaction_id = helper::create_transaction_with_journal(
                    self,
                    services,
                    amount,
                    house_account.ledger_id,
                    LedgerAction::Withdraw,
                )
                .await?;

                // Debit hold: move balance to pending to prevent overdraft
                let ledger_id = Uuid::parse_str(&self.ledger_id)
                    .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?;
                services
                    .services
                    .debit_hold(id, ledger_id, transaction_id, amount)
                    .await?;

                Ok(vec![])
            }
            BankAccountCommand::Transfer {
                id,
                to_account_id,
                amount,
            } => {
                // Validate source != destination
                if id == to_account_id {
                    return Err("cannot transfer to the same account".into());
                }

                // Validate destination account exists and is Approved
                let dest_account = services
                    .services
                    .get_bank_account(to_account_id)
                    .await
                    .map_err(|_| "destination account not found")?;

                if dest_account.status != models::BankAccountStatus::Approved {
                    return Err("destination account is not active".into());
                }

                // Validate same currency
                if self.currency != dest_account.currency {
                    return Err("currency mismatch between source and destination".into());
                }

                // Create transfer transactions (source debit + destination credit)
                let dest_ledger_id = dest_account.ledger_id.clone();
                let transaction_id = helper::create_transfer_transactions(
                    self,
                    services,
                    to_account_id,
                    dest_ledger_id,
                    amount,
                )
                .await?;

                // Debit hold on source: move balance to pending
                let ledger_id = Uuid::parse_str(&self.ledger_id)
                    .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?;
                services
                    .services
                    .debit_hold(id, ledger_id, transaction_id, amount)
                    .await?;

                Ok(vec![])
            }
        }
    }

    fn apply(&mut self, event: Self::Event) {
        match event {
            events::BankAccountEvent::AccountOpened {
                base_event,
                account_type,
                kind,
                external_reference_id,
                account_number,
                currency,
            } => {
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Pending;
                self.timestamp = base_event.get_created_at();
                self.account_type = account_type;
                self.kind = kind;
                self.currency = currency;
                self.external_reference_id = external_reference_id;
                self.account_number = account_number;
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
            events::BankAccountEvent::AccountFrozen { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Freeze;
                self.timestamp = base_event.get_created_at();
            }
            events::BankAccountEvent::AccountUnfrozen { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::Approved;
                self.timestamp = base_event.get_created_at();
            }
            events::BankAccountEvent::AccountClosed { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = models::BankAccountStatus::CustomerClosed;
                self.timestamp = base_event.get_created_at();
            }
            events::BankAccountEvent::CustomerDepositedCash { .. } => {}
            events::BankAccountEvent::CustomerWithdrewCash { .. } => {}
        }
    }
}

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
        service::{BankAccountApi, BankAccountServices},
    };

    use super::{
        command::{BankAccountCommand, LedgerCommand},
        event::{BaseEvent, Event},
        events::BankAccountEvent,
        finance::{JournalEntry, JournalLine, Transaction},
        models::{
            BankAccount, BankAccountKind, BankAccountStatus, BankAccountType, BankAccountView,
            HouseAccount, LedgerAction,
        },
    };

    type AccountTestFramework = TestFramework<BankAccount>;

    lazy_static! {
        static ref LEDGER_ID: Uuid = Uuid::new_v4();
        static ref ACCOUNT_ID: Uuid = Uuid::new_v4();
        static ref TRANSACTION_ID: Uuid = Uuid::new_v4();
        static ref DEST_ACCOUNT_ID: Uuid = Uuid::new_v4();
    }

    fn create_base_event(uuid: Uuid) -> BaseEvent {
        let mut base_event = BaseEvent::default();
        base_event.set_aggregate_id(uuid);
        base_event.set_created_at(chrono::Utc::now());
        base_event
    }

    fn approved_account_events() -> Vec<BankAccountEvent> {
        vec![
            BankAccountEvent::AccountOpened {
                base_event: create_base_event(*ACCOUNT_ID),
                account_type: BankAccountType::Retail,
                kind: BankAccountKind::Checking,
                external_reference_id: Some("user".to_string()),
                account_number: "123456789012".to_string(),
                currency: Currency::USD,
            },
            BankAccountEvent::AccountKycApproved {
                ledger_id: LEDGER_ID.to_string(),
                base_event: create_base_event(*ACCOUNT_ID),
            },
        ]
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

    macro_rules! test_error_case {
        ($name:ident, $given:expr, $command:expr, $expected_err:expr) => {
            #[test]
            fn $name() {
                let services = BankAccountServices::new(Box::new(setup_mock_services()));
                AccountTestFramework::with(services)
                    .given($given)
                    .when($command)
                    .then_expect_error_message($expected_err);
            }
        };
    }

    test_case!(
        test_account_creation,
        vec![],
        BankAccountCommand::OpenAccount {
            id: *ACCOUNT_ID,
            parent_id: None,
            account_number: "123456789012".to_string(),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: Some("user".to_string()),
            currency: Currency::USD
        },
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: Some("user".to_string()),
            account_number: "123456789012".to_string(),
            currency: Currency::USD
        }]
    );

    test_case!(
        test_account_creation_no_external_ref,
        vec![],
        BankAccountCommand::OpenAccount {
            id: *ACCOUNT_ID,
            parent_id: None,
            account_number: "123456789012".to_string(),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: None,
            currency: Currency::USD
        },
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: None,
            account_number: "123456789012".to_string(),
            currency: Currency::USD
        }]
    );

    test_case!(
        test_account_kyc_approved,
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: Some("user".to_string()),
            account_number: "123456789012".to_string(),
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
        test_freeze_account,
        approved_account_events(),
        BankAccountCommand::FreezeAccount { id: *ACCOUNT_ID },
        vec![BankAccountEvent::AccountFrozen {
            base_event: create_base_event(*ACCOUNT_ID)
        }]
    );

    test_case!(
        test_unfreeze_account,
        {
            let mut events = approved_account_events();
            events.push(BankAccountEvent::AccountFrozen {
                base_event: create_base_event(*ACCOUNT_ID),
            });
            events
        },
        BankAccountCommand::UnfreezeAccount { id: *ACCOUNT_ID },
        vec![BankAccountEvent::AccountUnfrozen {
            base_event: create_base_event(*ACCOUNT_ID)
        }]
    );

    test_error_case!(
        test_freeze_pending_account_fails,
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: Some("user".to_string()),
            account_number: "123456789012".to_string(),
            currency: Currency::USD
        }],
        BankAccountCommand::FreezeAccount { id: *ACCOUNT_ID },
        "account must be Approved to freeze"
    );

    test_error_case!(
        test_unfreeze_approved_account_fails,
        approved_account_events(),
        BankAccountCommand::UnfreezeAccount { id: *ACCOUNT_ID },
        "account must be Frozen to unfreeze"
    );

    test_case!(
        test_close_account,
        approved_account_events(),
        BankAccountCommand::CloseAccount { id: *ACCOUNT_ID },
        vec![BankAccountEvent::AccountClosed {
            base_event: create_base_event(*ACCOUNT_ID)
        }]
    );

    test_error_case!(
        test_close_pending_account_fails,
        vec![BankAccountEvent::AccountOpened {
            base_event: create_base_event(*ACCOUNT_ID),
            account_type: BankAccountType::Retail,
            kind: BankAccountKind::Checking,
            external_reference_id: Some("user".to_string()),
            account_number: "123456789012".to_string(),
            currency: Currency::USD
        }],
        BankAccountCommand::CloseAccount { id: *ACCOUNT_ID },
        "account must be Approved to close"
    );

    test_case!(
        test_deposit,
        approved_account_events(),
        BankAccountCommand::Deposit {
            id: *ACCOUNT_ID,
            amount: Money::new(dec!(1000.0), Currency::USD)
        },
        vec![]
    );

    test_case!(
        test_withdrawal,
        approved_account_events(),
        BankAccountCommand::Withdrawal {
            id: *ACCOUNT_ID,
            amount: Money::new(dec!(500.0), Currency::USD)
        },
        vec![]
    );

    test_case!(
        test_transfer,
        approved_account_events(),
        BankAccountCommand::Transfer {
            id: *ACCOUNT_ID,
            to_account_id: *DEST_ACCOUNT_ID,
            amount: Money::new(dec!(100.0), Currency::USD)
        },
        vec![]
    );

    test_error_case!(
        test_transfer_to_self_fails,
        approved_account_events(),
        BankAccountCommand::Transfer {
            id: *ACCOUNT_ID,
            to_account_id: *ACCOUNT_ID,
            amount: Money::new(dec!(100.0), Currency::USD)
        },
        "cannot transfer to the same account"
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

        async fn create_transaction_with_journal(
            &self,
            _transaction: Transaction,
            _ledger_id: String,
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
            _asset_code: &str,
        ) -> Result<HouseAccount, anyhow::Error> {
            Ok(HouseAccount::default())
        }

        async fn validate_account_creation(
            &self,
            _account_id: Uuid,
            _external_reference_id: Option<String>,
            _currency: Currency,
            _kind: BankAccountKind,
        ) -> Result<bool, anyhow::Error> {
            Ok(true)
        }

        async fn find_checking_account(
            &self,
            _external_reference_id: Option<String>,
            _currency: Currency,
        ) -> Result<Option<Uuid>, anyhow::Error> {
            Ok(None)
        }

        async fn get_bank_account(
            &self,
            _account_id: Uuid,
        ) -> Result<BankAccountView, anyhow::Error> {
            Ok(BankAccountView {
                status: BankAccountStatus::Approved,
                ..Default::default()
            })
        }

        async fn debit_hold(
            &self,
            _account_id: Uuid,
            _ledger_id: Uuid,
            _transaction_id: Uuid,
            _amount: Money,
        ) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn get_ledger_balance(
            &self,
            _account_id: Uuid,
        ) -> Result<(Money, Money), anyhow::Error> {
            Ok((
                Money::new(Decimal::ZERO, Currency::USD),
                Money::new(Decimal::ZERO, Currency::USD),
            ))
        }

        async fn create_transfer_transactions(
            &self,
            _source_account_id: Uuid,
            _source_ledger_id: String,
            _dest_account_id: Uuid,
            _dest_ledger_id: String,
            _amount: Money,
        ) -> Result<Uuid, anyhow::Error> {
            Ok(Uuid::new_v4())
        }
    }
}
