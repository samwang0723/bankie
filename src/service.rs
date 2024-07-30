use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::{event_sourcing::command::BalanceCommand, state::BalanceLoaderSaver};

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
}

pub struct BankAccountLogic {
    pub ledger: BalanceLoaderSaver,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn write_balance(
        &self,
        id: String,
        command: BalanceCommand,
    ) -> Result<(), anyhow::Error> {
        // Should call ledger commange to write the transaction.
        self.ledger
            .cqrs
            .execute(&id, command)
            .await
            .map_err(|e| anyhow!("Failed to write ledger: {}", e))
    }
}
