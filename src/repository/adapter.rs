use async_trait::async_trait;
use mockall::automock;
use sqlx::Error;
use uuid::Uuid;

use crate::domain::{
    finance::{JournalEntry, JournalLine, Outbox, Transaction},
    models::{BankAccountKind, HouseAccount},
    tenant::Tenant,
    user::BankAccountWithLedger,
};

#[automock]
#[async_trait]
pub trait DatabaseClient {
    async fn get_user_bank_accounts(
        &self,
        user_id: String,
    ) -> Result<Vec<BankAccountWithLedger>, Error>;
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
    async fn get_house_account(&self, asset_code: &str) -> Result<HouseAccount, Error>;
    async fn get_house_accounts(&self, asset_code: &str) -> Result<Vec<HouseAccount>, Error>;
    async fn validate_bank_account_exists(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        kind: BankAccountKind,
    ) -> Result<bool, Error>;
    async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
    ) -> Result<Option<String>, Error>;
    async fn get_sub_accounts(
        &self,
        account_id: String,
    ) -> Result<Vec<BankAccountWithLedger>, Error>;
    async fn get_bank_account_by_number(
        &self,
        account_number: String,
    ) -> Result<BankAccountWithLedger, Error>;
    async fn create_tenant_profile(&self, name: &str, scope: &str) -> Result<i32, Error>;
    async fn update_tenant_profile(&self, id: i32, jwt: &str) -> Result<i32, Error>;
    async fn get_tenant_profile(&self, tenant_id: i32) -> Result<Tenant, Error>;
    async fn get_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error>;
    async fn get_transactions(
        &self,
        bank_account_id: String,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Transaction>, Error>;
    async fn move_to_dead_letter(
        &self,
        outbox_id: i32,
        transaction_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
        error_message: &str,
        retry_count: i32,
    ) -> Result<(), Error>;
    async fn increment_outbox_retry(
        &self,
        outbox_id: i32,
        error_message: &str,
    ) -> Result<(), Error>;
    async fn load_assets(&self) -> Result<Vec<crate::common::asset::Asset>, Error>;
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

    pub async fn get_house_account(&self, asset_code: &str) -> Result<HouseAccount, Error> {
        self.client.get_house_account(asset_code).await
    }

    pub async fn get_house_accounts(&self, asset_code: &str) -> Result<Vec<HouseAccount>, Error> {
        self.client.get_house_accounts(asset_code).await
    }

    pub async fn validate_bank_account_exists(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        kind: BankAccountKind,
    ) -> Result<bool, Error> {
        self.client
            .validate_bank_account_exists(external_reference_id, currency, kind)
            .await
    }

    pub async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
    ) -> Result<Option<String>, Error> {
        self.client
            .find_checking_account(external_reference_id, currency)
            .await
    }

    pub async fn get_sub_accounts(
        &self,
        account_id: String,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_sub_accounts(account_id).await
    }

    pub async fn get_bank_account_by_number(
        &self,
        account_number: String,
    ) -> Result<BankAccountWithLedger, Error> {
        self.client.get_bank_account_by_number(account_number).await
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

    pub async fn get_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error> {
        self.client.get_unprocessed_outbox().await
    }

    pub async fn get_user_bank_accounts(
        &self,
        user_id: String,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_user_bank_accounts(user_id).await
    }

    pub async fn get_transactions(
        &self,
        bank_account_id: String,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Transaction>, Error> {
        self.client
            .get_transactions(bank_account_id, offset, limit)
            .await
    }

    pub async fn move_to_dead_letter(
        &self,
        outbox_id: i32,
        transaction_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
        error_message: &str,
        retry_count: i32,
    ) -> Result<(), Error> {
        self.client
            .move_to_dead_letter(
                outbox_id,
                transaction_id,
                event_type,
                payload,
                error_message,
                retry_count,
            )
            .await
    }

    pub async fn increment_outbox_retry(
        &self,
        outbox_id: i32,
        error_message: &str,
    ) -> Result<(), Error> {
        self.client
            .increment_outbox_retry(outbox_id, error_message)
            .await
    }

    pub async fn load_assets(&self) -> Result<Vec<crate::common::asset::Asset>, Error> {
        self.client.load_assets().await
    }
}
