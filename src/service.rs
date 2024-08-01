use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    common::money::Money,
    domain::{
        finance::{JournalEntry, JournalLine, Transaction},
        models::{BankAccountStatus, LedgerAction},
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
    async fn note_ledger(&self, id: String, command: LedgerCommand) -> Result<(), anyhow::Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, anyhow::Error>;
    async fn validate(
        &self,
        account_id: Uuid,
        action: LedgerAction,
        amount: Money,
    ) -> Result<(), anyhow::Error>;
}

pub struct BankAccountLogic {
    pub bank_account: BankAccountLoader,
    pub ledger: LedgerLoaderSaver,
    pub finance: Arc<Adapter<PgPool>>,
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
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, anyhow::Error> {
        self.finance
            .create_transaction_with_journal(transaction, journal_entry, journal_lines)
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
                None => println!("Account not found"),
                Some(account_view) => {
                    if account_view.status != BankAccountStatus::Approved {
                        return Err(anyhow!("Account is not active"));
                    }
                    if account_view.currency != amount.currency {
                        return Err(anyhow!("Currency invalid"));
                    }

                    match self.ledger.query.load(&account_view.ledger_id).await {
                        Ok(view) => match view {
                            None => println!("Ledger not found"),
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
}
