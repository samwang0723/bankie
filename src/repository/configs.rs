use cqrs_es::{persist::PersistedEventStore, CqrsFramework, Query};
use postgres_es::{PostgresCqrs, PostgresEventRepository, PostgresViewRepository};
use sqlx::{Pool, Postgres};
use std::sync::Arc;

use crate::{
    domain::models::*,
    service::{BankAccountLogic, BankAccountServices, MockLedgerServices},
    state::LedgerLoaderSaver,
};

use super::{
    account_view::{AccountLogging, AccountQuery},
    ledger_view::{LedgerLogging, LedgerQuery},
};

pub fn configure_bank_account(
    pool: Pool<Postgres>,
    ledger_loader_saver: LedgerLoaderSaver,
) -> (
    Arc<PostgresCqrs<BankAccount>>,
    Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
) {
    // A very simple query that writes each event to stdout.
    let logging_query = AccountLogging {};

    // A query that stores the current state of an individual account.
    let account_view_repo = Arc::new(PostgresViewRepository::new("account_views", pool.clone()));
    let mut account_query = AccountQuery::new(account_view_repo.clone());

    // Without a query error handler there will be no indication if an
    // error occurs (e.g., database connection failure, missing columns or table).
    // Consider logging an error or panicking in your own application.
    account_query.use_error_handler(Box::new(|e| println!("{}", e)));

    // Create and return an event-sourced `CqrsFramework`.
    let queries: Vec<Box<dyn Query<BankAccount>>> =
        vec![Box::new(logging_query), Box::new(account_query)];
    let services = BankAccountServices::new(Box::new(BankAccountLogic {
        ledger: ledger_loader_saver,
    }));

    let repo = PostgresEventRepository::new(pool)
        .with_tables("bank_account_events", "bank_account_snapshots");
    let store = PersistedEventStore::new_event_store(repo);
    let cqrs = CqrsFramework::new(store, queries, services);

    (Arc::new(cqrs), account_view_repo)
}

pub fn configure_ledger(
    pool: Pool<Postgres>,
) -> (
    Arc<PostgresCqrs<Ledger>>,
    Arc<PostgresViewRepository<LedgerView, Ledger>>,
) {
    // A very simple query that writes each event to stdout.
    let logging_query = LedgerLogging {};

    // A query that stores the current state of an individual account.
    let ledger_view_repo = Arc::new(PostgresViewRepository::new("ledger_views", pool.clone()));
    let mut ledger_query = LedgerQuery::new(ledger_view_repo.clone());

    // Without a query error handler there will be no indication if an
    // error occurs (e.g., database connection failure, missing columns or table).
    // Consider logging an error or panicking in your own application.
    ledger_query.use_error_handler(Box::new(|e| println!("{}", e)));

    // Create and return an event-sourced `CqrsFramework`.
    let queries: Vec<Box<dyn Query<Ledger>>> =
        vec![Box::new(logging_query), Box::new(ledger_query)];

    let repo = PostgresEventRepository::new(pool).with_tables("ledger_events", "ledger_snapshots");
    let store = PersistedEventStore::new_event_store(repo);
    let cqrs = CqrsFramework::new(store, queries, MockLedgerServices {});

    (Arc::new(cqrs), ledger_view_repo)
}
