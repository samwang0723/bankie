use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::{event_sourcing::command::LedgerCommand, state::LedgerLoaderSaver};

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
    async fn write_ledger(
        &self,
        ledger_id: String,
        command: LedgerCommand,
    ) -> Result<(), anyhow::Error>;
}

pub struct BankAccountLogic {
    pub ledger: LedgerLoaderSaver,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn write_ledger(
        &self,
        ledger_id: String,
        command: LedgerCommand,
    ) -> Result<(), anyhow::Error> {
        // Should call ledger commange to write the transaction.
        self.ledger
            .cqrs
            .execute(&ledger_id, command)
            .await
            .map_err(|e| anyhow!("Failed to write ledger: {}", e))
    }
}
