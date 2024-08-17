use async_trait::async_trait;
use mockall::automock;
use sqlx::Error;
use uuid::Uuid;

use crate::{
    common::money::Currency,
    domain::{
        finance::{JournalEntry, JournalLine, Outbox, Transaction},
        models::{BankAccountKind, HouseAccount},
        tenant::Tenant,
    },
};

#[automock]
#[async_trait]
pub trait DatabaseClient {
    async fn fail_transaction(&self, transaction_id: Uuid) -> Result<(), Error>;
    async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, Error>;
    async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error>;
    async fn get_house_account(&self, currency: Currency) -> Result<HouseAccount, Error>;
    async fn get_house_accounts(&self, currency: Currency) -> Result<Vec<HouseAccount>, Error>;
    async fn validate_bank_account_exists(
        &self,
        user_id: String,
        currency: Currency,
        kind: BankAccountKind,
    ) -> Result<bool, Error>;
    async fn create_tenant_profile(&self, name: &str, scope: &str) -> Result<i32, Error>;
    async fn update_tenant_profile(&self, id: i32, jwt: &str) -> Result<i32, Error>;
    async fn get_tenant_profile(&self, tenant_id: i32) -> Result<Tenant, Error>;
    async fn fetch_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error>;
}

pub struct Adapter<C: DatabaseClient + Send + Sync> {
    client: C,
}

impl<C: DatabaseClient + Send + Sync> Adapter<C> {
    pub fn new(client: C) -> Self {
        Adapter { client }
    }

    pub async fn fail_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        self.client.fail_transaction(transaction_id).await
    }

    pub async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error> {
        self.client.complete_transaction(transaction_id).await
    }

    pub async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
    ) -> Result<Uuid, Error> {
        self.client
            .create_transaction_with_journal(transaction, ledger_id, journal_entry, journal_lines)
            .await
    }

    pub async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error> {
        self.client.create_house_account(account).await
    }

    pub async fn get_house_account(&self, currency: Currency) -> Result<HouseAccount, Error> {
        self.client.get_house_account(currency).await
    }

    pub async fn get_house_accounts(&self, currency: Currency) -> Result<Vec<HouseAccount>, Error> {
        self.client.get_house_accounts(currency).await
    }

    pub async fn validate_bank_account_exists(
        &self,
        user_id: String,
        currency: Currency,
        kind: BankAccountKind,
    ) -> Result<bool, Error> {
        self.client
            .validate_bank_account_exists(user_id, currency, kind)
            .await
    }

    pub async fn create_tenant_profile(&self, name: &str, scope: &str) -> Result<i32, Error> {
        self.client.create_tenant_profile(name, scope).await
    }

    pub async fn update_tenant_profile(&self, id: i32, jwt: &str) -> Result<i32, Error> {
        self.client.update_tenant_profile(id, jwt).await
    }

    pub async fn get_tenant_profile(&self, tenant_id: i32) -> Result<Tenant, Error> {
        self.client.get_tenant_profile(tenant_id).await
    }

    pub async fn fetch_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error> {
        self.client.fetch_unprocessed_outbox().await
    }
}
