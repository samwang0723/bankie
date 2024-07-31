use async_trait::async_trait;
use sqlx::Error;

use crate::domain::finance::{JournalEntry, JournalLine, Transaction};

#[async_trait]
pub trait DatabaseClient {
    async fn create_transaction(&self, transaction: Transaction) -> Result<Transaction, Error>;

    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Transaction, Error>;
}

pub struct Adapter<C: DatabaseClient + Send + Sync> {
    client: C,
}

impl<C: DatabaseClient + Send + Sync> Adapter<C> {
    pub fn new(client: C) -> Self {
        Adapter { client }
    }

    pub async fn create_transaction(&self, transaction: Transaction) -> Result<Transaction, Error> {
        self.client.create_transaction(transaction).await
    }

    pub async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Transaction, Error> {
        self.client
            .create_transaction_with_journal(transaction, journal_entry, journal_lines)
            .await
    }
}
