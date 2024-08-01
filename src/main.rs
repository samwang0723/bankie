use std::error::Error;

use common::money::{Currency, Money};
use cqrs_es::persist::ViewRepository;
use rust_decimal_macros::dec;
use state::{new_application_state, ApplicationState};
use uuid::Uuid;

mod common;
mod configs;
mod domain;
mod event_sourcing;
mod repository;
mod service;
mod state;

async fn create_bank_account(
    state: ApplicationState,
    user_id: String,
    account_id: Uuid,
    ledger_id: Uuid,
    account_type: domain::models::BankAccountType,
) -> Result<(), Box<dyn Error>> {
    // execute account opening
    let opening_command = event_sourcing::command::BankAccountCommand::OpenAccount {
        id: account_id,
        account_type,
        user_id,
        currency: Currency::USD,
    };

    state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), opening_command)
        .await?;

    // execute account KYC approved
    let approved_command = event_sourcing::command::BankAccountCommand::ApproveAccount {
        id: account_id,
        ledger_id,
    };

    state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), approved_command)
        .await?;

    Ok(())
}

/// While the beginning of the program, need to initialized two bank accounts
/// for double-entry accounting.
/// 1. Incoming Master Account
/// 2. Outgoing Master Account
///
/// ```
/// use state::{new_application_state, ApplicationState};
/// use uuid::Uuid;
///
/// dotenv::dotenv().ok();
/// let state = new_application_state().await;
///
/// // open incoming master account
/// create_bank_account(
///     state.clone(),
///     "".to_string(),
///     Uuid::new_v4(),
///     configs::settings::INCOMING_MASTER_BANK_UUID,
///     domain::models::BankAccountType::Master,
/// )
/// .await
/// .unwrap();
///
/// // open outgoing master account
/// create_bank_account(
///     state.clone(),
///     "".to_string(),
///     Uuid::new_v4(),
///     configs::settings::OUTGOING_MASTER_BANK_UUID,
///     domain::models::BankAccountType::Master,
/// )
/// .await
/// .unwrap();
/// ```
#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let state = new_application_state().await;
    let account_id = Uuid::new_v4();
    let ledger_id = Uuid::new_v4();

    // open customer account
    create_bank_account(
        state.clone(),
        Uuid::new_v4().to_string(),
        account_id,
        ledger_id,
        domain::models::BankAccountType::Retail,
    )
    .await
    .unwrap();

    // execute account deposit
    let deposit_command = event_sourcing::command::BankAccountCommand::Deposit {
        amount: Money::new(dec!(356.43), Currency::USD),
    };
    state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), deposit_command)
        .await
        .unwrap();

    // execute account Withdrawl
    let withdrawal_command = event_sourcing::command::BankAccountCommand::Withdrawl {
        amount: Money::new(dec!(26.23), Currency::USD),
    };
    state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), withdrawal_command)
        .await
        .unwrap();

    // read the account view
    match state.bank_account.query.load(&account_id.to_string()).await {
        Ok(view) => match view {
            None => println!("Account not found"),
            Some(account_view) => {
                println!("Account: {:#?}", account_view);
                println!("---------");
                // read the account view
                match state.ledger.query.load(&account_view.ledger_id).await {
                    Ok(view) => match view {
                        None => println!("Ledger not found"),
                        Some(ledger_view) => println!("Ledger: {:#?}", ledger_view),
                    },
                    Err(err) => {
                        println!("Error: {:#?}\n", err);
                    }
                };
            }
        },
        Err(err) => {
            println!("Error: {:#?}\n", err);
        }
    };
}
