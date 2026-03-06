use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use cqrs_es::persist::ViewRepository;
use rust_decimal::Decimal;
use tracing::error;
use uuid::Uuid;

use crate::{
    common::fx_rate::FxRateService,
    common::money::{Currency, Money},
    domain::{
        finance::{JournalEntry, JournalLine, Transaction},
        models::{BankAccountKind, BankAccountStatus, BankAccountView, HouseAccount, LedgerAction},
    },
    event_sourcing::command::LedgerCommand,
    repository::{adapter::Adapter, pools::DbPools},
    state::{BankAccountLoader, LedgerLoaderSaver},
};

pub struct MockLedgerServices;

pub struct BankAccountServices {
    pub services: Box<dyn BankAccountApi>,
    pub fx_rate_service: Option<Arc<FxRateService>>,
}

impl BankAccountServices {
    pub fn new(services: Box<dyn BankAccountApi>) -> Self {
        Self {
            services,
            fx_rate_service: None,
        }
    }

    pub fn with_fx_rate_service(mut self, service: Arc<FxRateService>) -> Self {
        self.fx_rate_service = Some(service);
        self
    }
}

// External services must be called during the processing of the command.
#[async_trait]
pub trait BankAccountApi: Sync + Send {
    async fn get_house_account(
        &self,
        asset_code: &str,
        tenant_id: i32,
    ) -> Result<HouseAccount, anyhow::Error>;
    async fn note_ledger(&self, id: String, command: LedgerCommand) -> Result<(), anyhow::Error>;
    async fn create_transaction_with_journal(
        &self,
        transaction: Transaction,
        ledger_id: String,
        journal_entry: JournalEntry,
        journal_lines: Vec<JournalLine>,
        tenant_id: i32,
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
        external_reference_id: Option<String>,
        currency: Currency,
        kind: BankAccountKind,
        tenant_id: i32,
    ) -> Result<bool, anyhow::Error>;
    async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: Currency,
        tenant_id: i32,
    ) -> Result<Option<Uuid>, anyhow::Error>;
    async fn get_bank_account(&self, account_id: Uuid) -> Result<BankAccountView, anyhow::Error>;
    async fn debit_hold(
        &self,
        account_id: Uuid,
        ledger_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
        tenant_id: i32,
    ) -> Result<(), anyhow::Error>;
    async fn get_ledger_balance(&self, account_id: Uuid) -> Result<(Money, Money), anyhow::Error>;
    #[allow(clippy::too_many_arguments)]
    async fn create_transfer_transactions(
        &self,
        source_account_id: Uuid,
        source_ledger_id: String,
        dest_account_id: Uuid,
        dest_ledger_id: String,
        amount: Money,
        tenant_id: i32,
        fx_conversion: Option<(Decimal, Decimal, String)>,
    ) -> Result<Uuid, anyhow::Error>;
}

pub struct BankAccountLogic {
    pub bank_account: BankAccountLoader,
    pub ledger: LedgerLoaderSaver,
    pub database: Arc<Adapter<DbPools>>,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn note_ledger(&self, id: String, command: LedgerCommand) -> Result<(), anyhow::Error> {
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
        tenant_id: i32,
    ) -> Result<Uuid, anyhow::Error> {
        self.database
            .create_transaction_with_journal(
                transaction,
                ledger_id,
                journal_entry,
                journal_lines,
                tenant_id,
            )
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
                                if (action == LedgerAction::Withdraw
                                    || action == LedgerAction::Transfer)
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

    async fn get_house_account(
        &self,
        asset_code: &str,
        tenant_id: i32,
    ) -> Result<HouseAccount, anyhow::Error> {
        self.database
            .get_house_account(asset_code, tenant_id)
            .await
            .map_err(|e| anyhow!("Failed to get house account: {}", e))
    }

    async fn validate_account_creation(
        &self,
        account_id: Uuid,
        external_reference_id: Option<String>,
        currency: Currency,
        kind: BankAccountKind,
        tenant_id: i32,
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

        // Only check for duplicates if external_reference_id is provided
        if let Some(ref ext_ref) = external_reference_id {
            let valid = self
                .database
                .validate_bank_account_exists(
                    Some(ext_ref.clone()),
                    &currency.to_string(),
                    kind,
                    tenant_id,
                )
                .await?;
            if !valid {
                return Err(anyhow!("Account duplicated"));
            }
        }

        Ok(true)
    }

    async fn find_checking_account(
        &self,
        external_reference_id: Option<String>,
        currency: Currency,
        tenant_id: i32,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        let ext_ref = match external_reference_id {
            Some(ref r) if !r.is_empty() => Some(r.clone()),
            _ => return Ok(None),
        };
        match self
            .database
            .find_checking_account(ext_ref, &currency.to_string(), tenant_id)
            .await?
        {
            Some(view_id) => {
                let uuid = Uuid::parse_str(&view_id)
                    .map_err(|e| anyhow!("Invalid checking account ID: {}", e))?;
                Ok(Some(uuid))
            }
            None => Ok(None),
        }
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
        tenant_id: i32,
    ) -> Result<(), anyhow::Error> {
        let cmd = LedgerCommand::DebitHold {
            id: ledger_id,
            account_id,
            transaction_id,
            amount,
            tenant_id,
        };
        match self.ledger.cqrs.execute(&ledger_id.to_string(), cmd).await {
            Ok(_) => Ok(()),
            Err(err) => Err(anyhow!("Failed to debit hold: {}", err)),
        }
    }

    async fn get_ledger_balance(&self, account_id: Uuid) -> Result<(Money, Money), anyhow::Error> {
        let account_view = self.get_bank_account(account_id).await?;
        match self.ledger.query.load(&account_view.ledger_id).await {
            Ok(Some(ledger_view)) => Ok((ledger_view.available, ledger_view.pending)),
            Ok(None) => {
                // No ledger yet — balance is zero
                let zero = Money::new(rust_decimal::Decimal::ZERO, account_view.currency);
                Ok((zero, zero))
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn create_transfer_transactions(
        &self,
        source_account_id: Uuid,
        source_ledger_id: String,
        dest_account_id: Uuid,
        dest_ledger_id: String,
        amount: Money,
        tenant_id: i32,
        fx_conversion: Option<(Decimal, Decimal, String)>,
    ) -> Result<Uuid, anyhow::Error> {
        self.database
            .create_transfer_transactions(
                source_account_id,
                source_ledger_id,
                dest_account_id,
                dest_ledger_id,
                amount,
                tenant_id,
                fx_conversion,
            )
            .await
            .map_err(|e| anyhow!("Failed to create transfer transactions: {}", e))
    }
}
