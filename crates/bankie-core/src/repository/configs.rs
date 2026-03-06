use cqrs_es::{persist::PersistedEventStore, CqrsFramework, Query};
use postgres_es::{PostgresCqrs, PostgresEventRepository, PostgresViewRepository};
use std::sync::Arc;
use tracing::error;

use crate::{
    common::fx_rate::FxRateService,
    domain::models::*,
    event_sourcing::query::{AccountLogging, AccountQuery, LedgerLogging, LedgerQuery},
    service::{BankAccountLogic, BankAccountServices, MockLedgerServices},
    state::{BankAccountLoader, LedgerLoaderSaver},
};

use super::adapter::Adapter;
use super::pools::DbPools;

pub fn configure_bank_account(
    pools: &DbPools,
    ledger_loader_saver: LedgerLoaderSaver,
    fx_rate_service: Option<Arc<FxRateService>>,
) -> (
    Arc<PostgresCqrs<BankAccount>>,
    Arc<PostgresViewRepository<BankAccountView, BankAccount>>,
) {
    let logging_query = AccountLogging {};

    // View repository reads from replica
    let account_view_repo = Arc::new(PostgresViewRepository::new(
        "bank_account_views",
        pools.read().clone(),
    ));
    let mut account_query = AccountQuery::new(account_view_repo.clone());
    account_query.use_error_handler(Box::new(|e| error!("{}", e)));

    let queries: Vec<Box<dyn Query<BankAccount>>> =
        vec![Box::new(logging_query), Box::new(account_query)];
    let mut services = BankAccountServices::new(Box::new(BankAccountLogic {
        bank_account: BankAccountLoader {
            query: Arc::clone(&account_view_repo),
        },
        ledger: ledger_loader_saver,
        database: Arc::new(Adapter::new(pools.clone())),
    }));
    if let Some(fx_service) = fx_rate_service {
        services = services.with_fx_rate_service(fx_service);
    }

    // Event repository writes to primary
    let repo = PostgresEventRepository::new(pools.write().clone())
        .with_tables("bank_account_events", "bank_account_snapshots");
    let store = PersistedEventStore::new_snapshot_store(repo, 3);
    let cqrs = CqrsFramework::new(store, queries, services);

    (Arc::new(cqrs), account_view_repo)
}

pub fn configure_ledger(
    pools: &DbPools,
) -> (
    Arc<PostgresCqrs<Ledger>>,
    Arc<PostgresViewRepository<LedgerView, Ledger>>,
) {
    let logging_query = LedgerLogging {};

    // View repository reads from replica
    let ledger_view_repo = Arc::new(PostgresViewRepository::new(
        "ledger_views",
        pools.read().clone(),
    ));
    let mut ledger_query = LedgerQuery::new(ledger_view_repo.clone());
    ledger_query.use_error_handler(Box::new(|e| error!("{}", e)));

    let queries: Vec<Box<dyn Query<Ledger>>> =
        vec![Box::new(logging_query), Box::new(ledger_query)];

    // Event repository writes to primary
    let repo = PostgresEventRepository::new(pools.write().clone())
        .with_tables("ledger_events", "ledger_snapshots");
    let store = PersistedEventStore::new_snapshot_store(repo, 3);
    let cqrs = CqrsFramework::new(store, queries, MockLedgerServices {});

    (Arc::new(cqrs), ledger_view_repo)
}
