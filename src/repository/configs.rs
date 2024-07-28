use cqrs_es::Query;
use postgres_es::{PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};
use std::sync::Arc;

use crate::{
    domain::models::*,
    service::{BankAccountServices, HappyPathBankAccountServices},
};

use super::account_view::{AccountQuery, LoggingQuery};

pub fn cqrs_framework(
    pool: Pool<Postgres>,
) -> (
    Arc<PostgresCqrs<BankAccount>>,
    Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
) {
    // A very simple query that writes each event to stdout.
    let logging_query = LoggingQuery {};

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
    let services = BankAccountServices::new(Box::new(HappyPathBankAccountServices));
    (
        Arc::new(postgres_es::postgres_cqrs(pool, queries, services)),
        account_view_repo,
    )
}
