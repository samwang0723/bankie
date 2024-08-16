use command::LedgerCommand;
use event::{BaseEvent, Event};
use finance::{JournalEntry, JournalLine, Transaction};
use models::{BankAccount, BankAccountKind, LedgerAction};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::money::{Currency, Money};
use crate::domain::*;
use crate::service::BankAccountServices;
use crate::{common, event_sourcing::*};

pub async fn validate_account_creation(
    services: &BankAccountServices,
    id: Uuid,
    user_id: String,
    currency: Currency,
    kind: BankAccountKind,
) -> Result<(), error::BankAccountError> {
    let valid = services
        .services
        .validate_account_creation(id, user_id, currency, kind)
        .await?;
    if !valid {
        return Err("validation failed".into());
    }
    Ok(())
}

pub fn create_base_event(id: Uuid) -> BaseEvent {
    let mut base_event = BaseEvent::default();
    base_event.set_aggregate_id(id);
    base_event.set_created_at(chrono::Utc::now());
    base_event
}

pub async fn init_ledger(
    services: &BankAccountServices,
    ledger_id: Uuid,
    account_id: Uuid,
    currency: Currency,
) -> Result<(), error::BankAccountError> {
    let command = LedgerCommand::Init {
        id: ledger_id,
        account_id,
        amount: Money::new(Decimal::ZERO, currency),
    };
    services
        .services
        .note_ledger(ledger_id.to_string(), command)
        .await
        .map_err(|_| "ledger write failed".into())
}

pub async fn validate_ledger_action(
    bank_account: &BankAccount,
    services: &BankAccountServices,
    action: LedgerAction,
    amount: Money,
) -> Result<(), error::BankAccountError> {
    services
        .services
        .validate(Uuid::parse_str(&bank_account.id).unwrap(), action, amount)
        .await
        .map_err(|_| "validation failed".into())
}

pub async fn create_transaction_with_journal(
    bank_account: &BankAccount,
    services: &BankAccountServices,
    amount: Money,
    house_account_ledger: String,
    action_type: LedgerAction,
) -> Result<Uuid, error::BankAccountError> {
    let transaction = Transaction {
        id: Uuid::new_v4(),
        bank_account_id: Uuid::parse_str(&bank_account.id).unwrap(),
        transaction_reference: common::snowflake::generate_transaction_reference("DE"),
        transaction_date: chrono::Utc::now().date_naive(),
        amount: amount.amount,
        currency: amount.currency.to_string(),
        description: None,
        journal_entry_id: None,
        status: "processing".to_string(),
    };

    let journal_entry = JournalEntry {
        id: Uuid::new_v4(),
        entry_date: chrono::Utc::now().date_naive(),
        description: None,
        status: "posted".to_string(),
    };
    let mut house_account_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: house_account_ledger,
        credit_amount: Decimal::ZERO,
        debit_amount: Decimal::ZERO,
        currency: amount.currency.to_string(),
        description: None,
    };
    let mut user_account_journal_line = JournalLine {
        id: Uuid::new_v4(),
        journal_entry_id: None,
        ledger_id: bank_account.ledger_id.clone(),
        debit_amount: Decimal::ZERO,
        credit_amount: Decimal::ZERO,
        currency: amount.currency.to_string(),
        description: None,
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
        .create_transaction_with_journal(transaction, journal_entry, journal_lines)
        .await
        .map_err(|_| "transaction update failed".into())
}

pub async fn note_ledger_and_complete_transaction(
    bank_account: &BankAccount,
    services: &BankAccountServices,
    transaction_id: Uuid,
    command: LedgerCommand,
) -> Result<Vec<events::BankAccountEvent>, error::BankAccountError> {
    if services
        .services
        .note_ledger(bank_account.ledger_id.clone(), command)
        .await
        .is_err()
    {
        services
            .services
            .fail_transaction(transaction_id)
            .await
            .map_err(|_| "transaction update failed")?;
        return Err("ledger write failed".into());
    }
    services
        .services
        .complete_transaction(transaction_id)
        .await
        .map_err(|_| "transaction update failed")?;
    Ok(vec![])
}
