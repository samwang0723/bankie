use common::money::{Currency, Money};
use cqrs_es::persist::ViewRepository;
use rust_decimal_macros::dec;
use state::new_application_state;
use std::collections::HashMap;
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
    let mut metadata: HashMap<String, String> = HashMap::new();
    metadata.insert("time".to_string(), chrono::Utc::now().to_rfc3339());
    match state
        .cqrs
        .execute_with_metadata(&account_id, opening_command, metadata)
        .await
    {
        Ok(_) => {
            println!("Account opened");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // execute deposit
    let deposit_command = event_sourcing::command::BankAccountCommand::DepositMoney {
        amount: Money::new(dec!(100.00), Currency::USD),
    };
    let mut metadata_2: HashMap<String, String> = HashMap::new();
    metadata_2.insert("time".to_string(), chrono::Utc::now().to_rfc3339());
    match state
        .cqrs
        .execute_with_metadata(&account_id, deposit_command, metadata_2)
        .await
    {
        Ok(_) => {
            println!("Deposit successful");
        }
        Err(e) => {
            println!("Error: {:?}", e);
        }
    }

    // read the account view
    match state.account_query.load(&account_id).await {
        Ok(view) => match view {
            None => println!("Account not found"),
            Some(account_view) => println!("Account: {:#?}", account_view),
        },
        Err(err) => {
            println!("Error: {:#?}\n", err);
        }
    };
}
