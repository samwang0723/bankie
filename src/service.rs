use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::finance::{JournalEntry, JournalLine, Transaction},
    event_sourcing::command::LedgerCommand,
    repository::adapter::Adapter,
    state::LedgerLoaderSaver,
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
}

pub struct BankAccountLogic {
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
}
