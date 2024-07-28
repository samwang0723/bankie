use std::sync::Arc;

use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};

use crate::configs::settings::SETTINGS;
use crate::domain::models::BankAccount;

use super::bank_account::BankAccountView;
use super::configs::cqrs_framework;

#[derive(Clone)]
pub struct ApplicationState {
    pub cqrs: Arc<PostgresCqrs<BankAccount>>,
    pub account_query: Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
}

pub async fn new_application_state() -> ApplicationState {
    // Configure the CQRS framework, backed by a Postgres database, along with two queries:
    // - a simply-query prints events to stdout as they are published
    // - `account_query` stores the current state of the account in a ViewRepository that we can access
    //
    // The needed database tables are automatically configured with `docker-compose up -d`,
    // see init file at `/db/init.sql` for more.
    let pool: Pool<Postgres> = default_postgress_pool(&SETTINGS.database.connection_string()).await;
    let (cqrs, account_query) = cqrs_framework(pool);
    ApplicationState {
        cqrs,
        account_query,
    }
}
