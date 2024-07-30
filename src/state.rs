use std::sync::Arc;

use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};

use crate::configs::settings::SETTINGS;
use crate::domain::models::{BankAccount, BankAccountView, Ledger, LedgerView};
use crate::repository::configs::{configure_bank_account, configure_ledger};

#[derive(Clone)]
pub struct ApplicationState {
    pub bank_account: BankAccountLoaderSaver,
    pub ledger: LedgerLoaderSaver,
}

#[derive(Clone)]
pub struct BankAccountLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<BankAccount>>,
    pub query: Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
}

#[derive(Clone)]
pub struct LedgerLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<Ledger>>,
    pub query: Arc<PostgresViewRepository<LedgerView, Ledger>>,
}

pub async fn new_application_state() -> ApplicationState {
    // Configure the CQRS framework, backed by a Postgres database, along with two queries:
    // - a simply-query prints events to stdout as they are published
    // - `account_query` stores the current state of the account in a ViewRepository that we can access
    //
    // The needed database tables are automatically configured with `docker-compose up -d`,
    // see init file at `/db/init.sql` for more.
    let pool: Pool<Postgres> = default_postgress_pool(&SETTINGS.database.connection_string()).await;
    let (ledger_cqrs, ledger_query) = configure_ledger(pool.clone());
    let ledger_loader_saver = LedgerLoaderSaver {
        cqrs: ledger_cqrs,
        query: ledger_query,
    };
    let (bc_cqrs, bc_query) = configure_bank_account(pool, ledger_loader_saver.clone());

    ApplicationState {
        bank_account: BankAccountLoaderSaver {
            cqrs: bc_cqrs,
            query: bc_query,
        },
        ledger: ledger_loader_saver,
    }
}
