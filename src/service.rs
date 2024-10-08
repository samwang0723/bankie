use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use crate::{
    common::money::{Currency, Money},
    domain::{
        finance::{JournalEntry, JournalLine, Transaction},
        models::{BankAccountKind, BankAccountStatus, BankAccountView, HouseAccount, LedgerAction},
    },
    event_sourcing::command::LedgerCommand,
    repository::adapter::Adapter,
    state::{BankAccountLoader, LedgerLoaderSaver},
};

pub struct MockLedgerServices;

pub struct BankAccountServices {
    pub services: Box<dyn BankAccountApi>,
}

impl BankAccountServices {
    pub fn new(services: Box<dyn BankAccountApi>) -> Self {
        Self { services }
    }
}

// External services must be called during the processing of the command.
#[async_trait]
pub trait BankAccountApi: Sync + Send {
    async fn get_house_account(&self, currency: Currency) -> Result<HouseAccount, anyhow::Error>;
    async fn note_ledger(&self, id: String, command: LedgerCommand) -> Result<(), anyhow::Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, anyhow::Error>;
    async fn validate(
        &self,
        account_id: Uuid,
        action: LedgerAction,
        amount: Money,
    ) -> Result<(), anyhow::Error>;
    async fn validate_account_creation(
        &self,
        account_id: Uuid,
        user_id: String,
        currency: Currency,
        kind: BankAccountKind,
    ) -> Result<bool, anyhow::Error>;
    async fn get_bank_account(&self, account_id: Uuid) -> Result<BankAccountView, anyhow::Error>;
    async fn debit_hold(
        &self,
        account_id: Uuid,
        ledger_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    ) -> Result<(), anyhow::Error>;
}

pub struct BankAccountLogic {
    pub bank_account: BankAccountLoader,
    pub ledger: LedgerLoaderSaver,
    pub database: Arc<Adapter<PgPool>>,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn note_ledger(&self, id: String, command: LedgerCommand) -> Result<(), anyhow::Error> {
        // Should call ledger commange to write the transaction.
        self.ledger
            .cqrs
            .execute(&id, command)
            .await
            .map_err(|e| anyhow!("Failed to write ledger: {}", e))
    }

    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, anyhow::Error> {
        self.database
            .create_transaction_with_journal(transaction, ledger_id, journal_entry, journal_lines)
            .await
            .map_err(|e| anyhow!("Failed to write transaction: {}", e))
    }

    async fn validate(
        &self,
        account_id: Uuid,
        action: LedgerAction,
        amount: Money,
    ) -> Result<(), anyhow::Error> {
        match self.bank_account.query.load(&account_id.to_string()).await {
            Ok(view) => match view {
                None => error!("Account not found"),
                Some(account_view) => {
                    if account_view.status != BankAccountStatus::Approved {
                        return Err(anyhow!("Account is not active"));
                    }
                    if account_view.currency != amount.currency {
                        return Err(anyhow!("Currency invalid"));
                    }

                    match self.ledger.query.load(&account_view.ledger_id).await {
                        Ok(view) => match view {
                            None => error!("Ledger not found"),
                            Some(ledger_view) => {
                                if action == LedgerAction::Withdraw
                                    && ledger_view.available < amount
                                {
                                    return Err(anyhow!("Insufficient funds"));
                                }
                            }
                        },
                        Err(err) => {
                            return Err(err.into());
                        }
                    };
                }
            },
            Err(err) => {
                return Err(err.into());
            }
        };

        Ok(())
    }

    async fn get_house_account(&self, currency: Currency) -> Result<HouseAccount, anyhow::Error> {
        self.database
            .get_house_account(currency)
            .await
            .map_err(|e| anyhow!("Failed to get house account: {}", e))
    }

    async fn validate_account_creation(
        &self,
        account_id: Uuid,
        user_id: String,
        currency: Currency,
        kind: BankAccountKind,
    ) -> Result<bool, anyhow::Error> {
        if (self
            .bank_account
            .query
            .load(&account_id.to_string())
            .await?)
            .is_some()
        {
            return Err(anyhow!("Account duplicated"));
        }

        let valid = self
            .database
            .validate_bank_account_exists(user_id, currency, kind)
            .await?;
        if !valid {
            return Err(anyhow!("Account duplicated"));
        }

        Ok(true)
    }

    async fn get_bank_account(&self, account_id: Uuid) -> Result<BankAccountView, anyhow::Error> {
        match self.bank_account.query.load(&account_id.to_string()).await {
            Ok(view) => match view {
                None => Err(anyhow!("Account not found")),
                Some(account_view) => Ok(account_view),
            },
            Err(err) => Err(err.into()),
        }
    }

    async fn debit_hold(
        &self,
        account_id: Uuid,
        ledger_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    ) -> Result<(), anyhow::Error> {
        let cmd = LedgerCommand::DebitHold {
            id: ledger_id,
            account_id,
            transaction_id,
            amount,
        };
        match self.ledger.cqrs.execute(&ledger_id.to_string(), cmd).await {
            Ok(_) => Ok(()),
            Err(err) => Err(anyhow!("Failed to debit hold: {}", err)),
        }
    }
}
