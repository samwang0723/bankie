use command::LedgerCommand;
use event::{BaseEvent, Event};
use finance::{
    JournalEntry, JournalLine, Transaction, TRANS_DEPOSIT, TRANS_TRANSFER, TRANS_WITHDRAWAL,
};
use models::{BankAccountKind, LedgerAction};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::service::BankAccountServices;
use crate::{common, event_sourcing::*};

pub async fn validate_account_creation(
    services: &BankAccountServices,
    id: Uuid,
    external_reference_id: Option<String>,
    currency: Currency,
    kind: BankAccountKind,
    tenant_id: i32,
) -> Result<(), error::BankAccountError> {
    let valid = services
        .services
        .validate_account_creation(id, external_reference_id, currency, kind, tenant_id)
        .await?;
    if !valid {
        return Err("validation failed".into());
    }
    Ok(())
}

pub fn create_base_event(id: Uuid, tenant_id: i32) -> BaseEvent {
    let mut base_event = BaseEvent::default();
    base_event.set_aggregate_id(id);
    base_event.set_created_at(chrono::Utc::now());
    base_event.set_tenant_id(tenant_id);
    base_event
}

pub async fn init_ledger(
    services: &BankAccountServices,
    ledger_id: Uuid,
    account_id: Uuid,
    currency: Currency,
    tenant_id: i32,
) -> Result<(), error::BankAccountError> {
    let command = LedgerCommand::Init {
        id: ledger_id,
        account_id,
        amount: Money::new(Decimal::ZERO, currency),
        tenant_id,
    };
    services
        .services
        .note_ledger(ledger_id.to_string(), command)
        .await
        .map_err(|_| "ledger write failed".into())
}

pub async fn create_transaction_with_journal(
    bank_account: &models::BankAccount,
    services: &BankAccountServices,
    amount: Money,
    house_account_ledger: String,
    action_type: LedgerAction,
    tenant_id: i32,
) -> Result<Uuid, error::BankAccountError> {
    // Validate ledger available is sufficient
    services
        .services
        .validate(
            Uuid::parse_str(&bank_account.id)
                .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?,
            action_type,
            amount,
        )
        .await?;

    let key = match action_type {
        LedgerAction::Deposit => TRANS_DEPOSIT,
        LedgerAction::Withdraw => TRANS_WITHDRAWAL,
        LedgerAction::Transfer => TRANS_TRANSFER,
    };
    let transaction = Transaction {
        id: Uuid::new_v4(),
        bank_account_id: Uuid::parse_str(&bank_account.id)
            .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?,
        transaction_reference: common::snowflake::generate_transaction_reference(key),
        transaction_date: chrono::Utc::now().date_naive(),
        amount: amount.amount,
        currency: amount.currency.to_string(),
        description: None,
        metadata: serde_json::Value::Null,
        journal_entry_id: None,
        status: "processing".to_string(),
        tenant_id,
    };

    let journal_entry = JournalEntry {
        id: Uuid::new_v4(),
        entry_date: chrono::Utc::now().date_naive(),
        description: None,
        status: "posted".to_string(),
        tenant_id,
    };
    let mut house_account_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: house_account_ledger,
        credit_amount: Decimal::ZERO,
        debit_amount: Decimal::ZERO,
        currency: amount.currency.to_string(),
        description: None,
        tenant_id,
    };
    let mut user_account_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: bank_account.ledger_id.clone(),
        debit_amount: Decimal::ZERO,
        credit_amount: Decimal::ZERO,
        currency: amount.currency.to_string(),
        description: None,
        tenant_id,
    };

    if action_type == LedgerAction::Deposit {
        house_account_journal_line.debit_amount = amount.amount;
        user_account_journal_line.credit_amount = amount.amount;
    } else {
        house_account_journal_line.credit_amount = amount.amount;
        user_account_journal_line.debit_amount = amount.amount;
    }

    let journal_lines = vec![house_account_journal_line, user_account_journal_line];
    services
        .services
        .create_transaction_with_journal(
            transaction,
            bank_account.ledger_id.clone(),
            journal_entry,
            journal_lines,
            tenant_id,
        )
        .await
        .map_err(|_| "transaction update failed".into())
}

/// Create transfer transactions: source debit + destination credit with double-entry journal.
/// Returns the source transaction ID for debit-hold.
pub async fn create_transfer_transactions(
    source_account: &models::BankAccount,
    services: &BankAccountServices,
    dest_account_id: Uuid,
    dest_ledger_id: String,
    amount: Money,
    tenant_id: i32,
) -> Result<Uuid, error::BankAccountError> {
    // Validate source has sufficient funds
    services
        .services
        .validate(
            Uuid::parse_str(&source_account.id)
                .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?,
            LedgerAction::Transfer,
            amount,
        )
        .await?;

    let source_account_id = Uuid::parse_str(&source_account.id)
        .map_err(|e| error::BankAccountError::from(e.to_string().as_str()))?;

    services
        .services
        .create_transfer_transactions(
            source_account_id,
            source_account.ledger_id.clone(),
            dest_account_id,
            dest_ledger_id,
            amount,
            tenant_id,
        )
        .await
        .map_err(|_| "transfer transaction creation failed".into())
}
