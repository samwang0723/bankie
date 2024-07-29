use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::fmt;

use crate::{
    common::money::Money, event_sourcing::command::LedgerCommand, state::LedgerLoaderSaver,
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

pub enum LedgerType {
    Init,
    Credit,
    Debit,
}

impl fmt::Display for LedgerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerType::Init => write!(f, "Init"),
            LedgerType::Credit => write!(f, "Credit"),
            LedgerType::Debit => write!(f, "Debit"),
        }
    }
}

// External services must be called during the processing of the command.
#[async_trait]
pub trait BankAccountApi: Sync + Send {
    async fn write_ledger(
        &self,
        ledger_id: &str,
        account_id: &str,
        amount: Money,
        ledger_type: LedgerType,
    ) -> Result<(), anyhow::Error>;
}

pub struct BankAccountLogic {
    pub ledger: LedgerLoaderSaver,
}

#[async_trait]
impl BankAccountApi for BankAccountLogic {
    async fn write_ledger(
        &self,
        ledger_id: &str,
        account_id: &str,
        amount: Money,
        ledger_type: LedgerType,
    ) -> Result<(), anyhow::Error> {
        // Should call ledger commange to write the transaction.
        let command = match ledger_type {
            LedgerType::Init => LedgerCommand::Init {
                ledger_id: ledger_id.to_string(),
                account_id: account_id.to_string(),
            },
            LedgerType::Credit => LedgerCommand::Credit {
                ledger_id: ledger_id.to_string(),
                account_id: account_id.to_string(),
                amount,
            },
            LedgerType::Debit => LedgerCommand::Debit {
                ledger_id: ledger_id.to_string(),
                account_id: account_id.to_string(),
                amount,
            },
        };

        self.ledger
            .cqrs
            .execute(ledger_id, command)
            .await
            .map_err(|e| anyhow!("Failed to write ledger: {}", e))
    }
}
