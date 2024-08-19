use std::sync::Arc;

use postgres_es::{default_postgress_pool, PostgresCqrs, PostgresViewRepository};
use sqlx::PgPool;
use tokio::sync::mpsc::UnboundedSender;

use crate::configs::settings::SETTINGS;
use crate::domain::models::{BankAccount, BankAccountView, Ledger, LedgerView};
use crate::event_sourcing::command::BankAccountCommand;
use crate::repository::adapter::{Adapter, DatabaseClient};
use crate::repository::configs::{configure_bank_account, configure_ledger};
use crate::SharedState;

#[derive(Clone)]
pub struct ApplicationState<C: DatabaseClient + Send + Sync> {
    pub bank_account: Option<BankAccountLoaderSaver>,
    pub ledger: Option<LedgerLoaderSaver>,
    pub database: Arc<Adapter<C>>,
    pub cache: Option<Arc<redis::Client>>,
    pub command_sender: Option<Arc<UnboundedSender<BankAccountCommand>>>,
}

impl<C: DatabaseClient + Send + Sync> ApplicationState<C> {
    pub fn new(database: Adapter<C>) -> Self {
        Self {
            bank_account: None,
            ledger: None,
            database: Arc::new(database),
            cache: None,
            command_sender: None,
        }
    }

    pub fn with_cache(mut self, cache: redis::Client) -> Self {
        self.cache = Some(Arc::new(cache));
        self
    }

    pub fn with_bank_account(mut self, bank_account: BankAccountLoaderSaver) -> Self {
        self.bank_account = Some(bank_account);
        self
    }

    pub fn with_ledger(mut self, ledger: LedgerLoaderSaver) -> Self {
        self.ledger = Some(ledger);
        self
    }

    pub fn with_command_sender(mut self, sender: UnboundedSender<BankAccountCommand>) -> Self {
        self.command_sender = Some(Arc::new(sender));
        self
    }
}

#[derive(Clone)]
pub struct BankAccountLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<BankAccount>>,
    pub query: Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
}

#[derive(Clone)]
pub struct BankAccountLoader {
    pub query: Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
}

#[derive(Clone)]
pub struct LedgerLoaderSaver {
    pub cqrs: Arc<PostgresCqrs<Ledger>>,
    pub query: Arc<PostgresViewRepository<LedgerView, Ledger>>,
}

pub async fn new_application_state(tx: UnboundedSender<BankAccountCommand>) -> SharedState {
    // Configure the CQRS framework, backed by a Postgres database, along with two queries:
    // - a simply-query prints events to stdout as they are published
    // - `query` stores the current state of the account in a ViewRepository that we can access
    let pool: PgPool = default_postgress_pool(&SETTINGS.database.connection_string()).await;
    let (ledger_cqrs, ledger_query) = configure_ledger(pool.clone());
    let ledger_loader_saver = LedgerLoaderSaver {
        cqrs: ledger_cqrs,
        query: ledger_query,
    };
    let (bc_cqrs, bc_query) = configure_bank_account(pool.clone(), ledger_loader_saver.clone());

    let cache = redis::Client::open(SETTINGS.redis.connection_string()).unwrap();

    Arc::new(
        ApplicationState::<PgPool>::new(Adapter::new(pool))
            .with_cache(cache)
            .with_bank_account(BankAccountLoaderSaver {
                cqrs: bc_cqrs,
                query: bc_query,
            })
            .with_ledger(ledger_loader_saver)
            .with_command_sender(tx),
    )
}
