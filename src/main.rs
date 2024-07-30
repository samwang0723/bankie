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
    let account_id = Uuid::new_v4().to_string();

    // execute account opening
    let opening_command = event_sourcing::command::BankAccountCommand::OpenAccount {
        account_id: account_id.clone(),
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id, opening_command)
        .await
    {
        Ok(_) => {
            println!("Account opened");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    let approved_command = event_sourcing::command::BankAccountCommand::ApproveAccount {
        account_id: account_id.clone(),
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id, approved_command)
        .await
    {
        Ok(_) => {
            println!("Account approved");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    let deposit_command = event_sourcing::command::BankAccountCommand::Deposit {
        amount: Money::new(dec!(356.43), Currency::USD),
    };
    match state
        .bank_account
        .cqrs
        .execute(&account_id, deposit_command)
        .await
    {
        Ok(_) => {
            println!("Account deposit money");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // read the account view
    match state.bank_account.query.load(&account_id).await {
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
