use std::sync::Arc;

use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::{Pool, Postgres};

use crate::configs::settings::SETTINGS;
use crate::domain::models::{Balance, BalanceView, BankAccount, BankAccountView};
use crate::repository::configs::{configure_bank_account, configure_ledger};

#[derive(Clone)]
pub struct ApplicationState {
    pub bank_account: BankAccountLoaderSaver,
    pub balance: BalanceLoaderSaver,
}

#[derive(Clone)]
pub struct BankAccountLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<BankAccount>>,
    pub query: Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
}

#[derive(Clone)]
pub struct BalanceLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<Balance>>,
    pub query: Arc<PostgresViewRepository<BalanceView, Balance>>,
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
    let ledger_loader_saver = BalanceLoaderSaver {
        cqrs: ledger_cqrs,
        query: ledger_query,
    };
    let (bc_cqrs, bc_query) = configure_bank_account(pool, ledger_loader_saver.clone());

    ApplicationState {
        bank_account: BankAccountLoaderSaver {
            cqrs: bc_cqrs,
            query: bc_query,
        },
        balance: ledger_loader_saver,
    }
}
