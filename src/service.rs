use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::finance::{JournalEntry, JournalLine, Transaction},
    event_sourcing::command::BalanceCommand,
    repository::adapter::Adapter,
    state::BalanceLoaderSaver,
};

pub struct MockBalanceServices;

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
    async fn write_balance(&self, id: String, command: BalanceCommand)
        -> Result<(), anyhow::Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, anyhow::Error>;
}

pub struct BankAccountLogic {
    pub balance: BalanceLoaderSaver,
    pub finance: Arc<Adapter<PgPool>>,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn write_balance(
        &self,
        id: String,
        command: BalanceCommand,
    ) -> Result<(), anyhow::Error> {
        // Should call ledger commange to write the transaction.
        self.balance
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
}
