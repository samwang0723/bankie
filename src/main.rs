use common::money::{Currency, Money};
use cqrs_es::persist::ViewRepository;
use rust_decimal_macros::dec;
use state::new_application_state;
use uuid::Uuid;

mod common;
mod configs;
mod domain;
mod event_sourcing;
mod repository;
mod service;
mod state;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    let state = new_application_state().await;
    let account_id = Uuid::new_v4();

    // execute account opening
    let opening_command =
        event_sourcing::command::BankAccountCommand::OpenAccount { id: account_id };
    match state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), opening_command)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // execute account KYC approved
    let balance_id = Uuid::new_v4();
    let approved_command = event_sourcing::command::BankAccountCommand::ApproveAccount {
        id: account_id,
        balance_id,
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), approved_command)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // execute account deposit
    let deposit_command = event_sourcing::command::BankAccountCommand::Deposit {
        amount: Money::new(dec!(356.43), Currency::USD),
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), deposit_command)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // execute account deposit
    let withdrawal_command = event_sourcing::command::BankAccountCommand::Withdrawl {
        amount: Money::new(dec!(26.23), Currency::USD),
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id.to_string(), withdrawal_command)
        .await
    {
        Ok(_) => {}
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // read the account view
    match state.bank_account.query.load(&account_id.to_string()).await {
        Ok(view) => match view {
            None => println!("Account not found"),
            Some(account_view) => {
                println!("Account: {:#?}", account_view);
                println!("---------");
                // read the account view
                match state.balance.query.load(&account_view.balance_id).await {
                    Ok(view) => match view {
                        None => println!("Balance not found"),
                        Some(ledger_view) => println!("Balance: {:#?}", ledger_view),
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
