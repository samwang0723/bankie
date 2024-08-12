use async_trait::async_trait;
use sqlx::Error;
use uuid::Uuid;

use crate::{
    common::money::Currency,
    domain::{
        finance::{JournalEntry, JournalLine, Transaction},
        models::HouseAccount,
    },
};

#[async_trait]
pub trait DatabaseClient {
    async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, Error>;
    async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error>;
    async fn get_house_account(&self, currency: Currency) -> Result<String, Error>;
}

pub struct Adapter<C: DatabaseClient + Send + Sync> {
    client: C,
}

impl<C: DatabaseClient + Send + Sync> Adapter<C> {
    pub fn new(client: C) -> Self {
        Adapter { client }
    }

    pub async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        self.client.complete_transaction(transaction_id).await
    }

    pub async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, Error> {
        self.client
            .create_transaction_with_journal(transaction, journal_entry, journal_lines)
            .await
    }

    pub async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error> {
        self.client.create_house_account(account).await
    }

    pub async fn get_house_account(&self, currency: Currency) -> Result<String, Error> {
        self.client.get_house_account(currency).await
    }
}
