use async_trait::async_trait;
use chrono::NaiveDate;
use mockall::automock;
use sqlx::Error;
use uuid::Uuid;

use rust_decimal::Decimal;

use crate::{
    common::money::Money,
    domain::{
        finance::{
            BalanceSnapshot, JournalEntry, JournalLine, Outbox, SettlementReportRow, Transaction,
        },
        models::{BankAccountKind, HouseAccount},
        tenant::Tenant,
        user::BankAccountWithLedger,
    },
};

#[allow(dead_code, clippy::too_many_arguments)]
#[automock]
#[async_trait]
pub trait DatabaseClient {
    async fn get_user_bank_accounts(
        &self,
        user_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error>;
    async fn fail_transaction(&self, transaction_id: Uuid) -> Result<(), Error>;
    async fn complete_transaction(&self, transaction_id: Uuid) -> Result<(), Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
        tenant_id: i32,
    ) -> Result<Uuid, Error>;
    async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error>;
    async fn get_house_account(
        &self,
        asset_code: &str,
        tenant_id: i32,
    ) -> Result<HouseAccount, Error>;
    async fn get_house_accounts(
        &self,
        asset_code: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<HouseAccount>, Error>;
    async fn validate_bank_account_exists(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        kind: BankAccountKind,
        tenant_id: i32,
    ) -> Result<bool, Error>;
    async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        tenant_id: i32,
    ) -> Result<Option<String>, Error>;
    async fn get_sub_accounts(
        &self,
        account_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error>;
    async fn get_bank_account_by_number(
        &self,
        account_number: String,
        tenant_id: i32,
    ) -> Result<BankAccountWithLedger, Error>;
    async fn create_tenant_profile(&self, name: &str, scope: &str) -> Result<i32, Error>;
    async fn update_tenant_profile(&self, id: i32, jwt: &str) -> Result<i32, Error>;
    async fn get_tenant_profile(&self, tenant_id: i32) -> Result<Tenant, Error>;
    async fn get_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error>;
    async fn get_transactions(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error>;
    async fn get_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error>;
    async fn count_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<i64, Error>;
    async fn create_transfer_transactions(
        &self,
        source_account_id: Uuid,
        source_ledger_id: String,
        dest_account_id: Uuid,
        dest_ledger_id: String,
        amount: Money,
        tenant_id: i32,
    ) -> Result<Uuid, Error>;
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
    async fn create_balance_snapshot(&self, snapshot: BalanceSnapshot) -> Result<(), Error>;
    async fn get_balance_history(
        &self,
        account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<BalanceSnapshot>, Error>;
    async fn get_all_active_account_balances(&self) -> Result<Vec<BankAccountWithLedger>, Error>;
    async fn get_settlement_report_data(
        &self,
        bank_account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<SettlementReportRow>, Error>;
    async fn get_opening_balance(
        &self,
        account_id: String,
        start_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Option<Decimal>, Error>;
    async fn get_accounts(
        &self,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error>;
    async fn count_accounts(&self, tenant_id: i32) -> Result<i64, Error>;
    async fn count_accounts_by_status(&self, tenant_id: i32) -> Result<Vec<(String, i64)>, Error>;
}

pub struct Adapter<C: DatabaseClient + Send + Sync> {
    client: C,
}

#[allow(dead_code, clippy::too_many_arguments)]
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
        tenant_id: i32,
    ) -> Result<Uuid, Error> {
        self.client
            .create_transaction_with_journal(
                transaction,
                ledger_id,
                journal_entry,
                journal_lines,
                tenant_id,
            )
            .await
    }

    pub async fn create_house_account(&self, account: HouseAccount) -> Result<(), Error> {
        self.client.create_house_account(account).await
    }

    pub async fn get_house_account(
        &self,
        asset_code: &str,
        tenant_id: i32,
    ) -> Result<HouseAccount, Error> {
        self.client.get_house_account(asset_code, tenant_id).await
    }

    pub async fn get_house_accounts(
        &self,
        asset_code: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<HouseAccount>, Error> {
        self.client.get_house_accounts(asset_code, tenant_id).await
    }

    pub async fn validate_bank_account_exists(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        kind: BankAccountKind,
        tenant_id: i32,
    ) -> Result<bool, Error> {
        self.client
            .validate_bank_account_exists(external_reference_id, currency, kind, tenant_id)
            .await
    }

    pub async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: &str,
        tenant_id: i32,
    ) -> Result<Option<String>, Error> {
        self.client
            .find_checking_account(external_reference_id, currency, tenant_id)
            .await
    }

    pub async fn get_sub_accounts(
        &self,
        account_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_sub_accounts(account_id, tenant_id).await
    }

    pub async fn get_bank_account_by_number(
        &self,
        account_number: String,
        tenant_id: i32,
    ) -> Result<BankAccountWithLedger, Error> {
        self.client
            .get_bank_account_by_number(account_number, tenant_id)
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

    pub async fn get_unprocessed_outbox(&self) -> Result<Vec<Outbox>, Error> {
        self.client.get_unprocessed_outbox().await
    }

    pub async fn get_user_bank_accounts(
        &self,
        user_id: String,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_user_bank_accounts(user_id, tenant_id).await
    }

    pub async fn get_transactions(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error> {
        self.client
            .get_transactions(bank_account_id, offset, limit, tenant_id)
            .await
    }

    pub async fn get_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        offset: i64,
        limit: i64,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<Vec<Transaction>, Error> {
        self.client
            .get_transactions_filtered(
                bank_account_id,
                offset,
                limit,
                start_date,
                end_date,
                transaction_type,
                status,
                tenant_id,
            )
            .await
    }

    pub async fn count_transactions_filtered(
        &self,
        bank_account_id: Option<String>,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
        transaction_type: Option<String>,
        status: Option<String>,
        tenant_id: i32,
    ) -> Result<i64, Error> {
        self.client
            .count_transactions_filtered(
                bank_account_id,
                start_date,
                end_date,
                transaction_type,
                status,
                tenant_id,
            )
            .await
    }

    pub async fn create_transfer_transactions(
        &self,
        source_account_id: Uuid,
        source_ledger_id: String,
        dest_account_id: Uuid,
        dest_ledger_id: String,
        amount: Money,
        tenant_id: i32,
    ) -> Result<Uuid, Error> {
        self.client
            .create_transfer_transactions(
                source_account_id,
                source_ledger_id,
                dest_account_id,
                dest_ledger_id,
                amount,
                tenant_id,
            )
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

    pub async fn create_balance_snapshot(&self, snapshot: BalanceSnapshot) -> Result<(), Error> {
        self.client.create_balance_snapshot(snapshot).await
    }

    pub async fn get_balance_history(
        &self,
        account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<BalanceSnapshot>, Error> {
        self.client
            .get_balance_history(account_id, start_date, end_date, tenant_id)
            .await
    }

    pub async fn get_all_active_account_balances(
        &self,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_all_active_account_balances().await
    }

    pub async fn get_settlement_report_data(
        &self,
        bank_account_id: String,
        start_date: NaiveDate,
        end_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Vec<SettlementReportRow>, Error> {
        self.client
            .get_settlement_report_data(bank_account_id, start_date, end_date, tenant_id)
            .await
    }

    pub async fn get_opening_balance(
        &self,
        account_id: String,
        start_date: NaiveDate,
        tenant_id: i32,
    ) -> Result<Option<Decimal>, Error> {
        self.client
            .get_opening_balance(account_id, start_date, tenant_id)
            .await
    }

    pub async fn get_accounts(
        &self,
        offset: i64,
        limit: i64,
        tenant_id: i32,
    ) -> Result<Vec<BankAccountWithLedger>, Error> {
        self.client.get_accounts(offset, limit, tenant_id).await
    }

    pub async fn count_accounts(&self, tenant_id: i32) -> Result<i64, Error> {
        self.client.count_accounts(tenant_id).await
    }

    pub async fn count_accounts_by_status(
        &self,
        tenant_id: i32,
    ) -> Result<Vec<(String, i64)>, Error> {
        self.client.count_accounts_by_status(tenant_id).await
    }
}
