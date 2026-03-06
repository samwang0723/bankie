use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

use crate::common::asset::AssetRegistry;
use crate::common::fx_rate::FxRateService;
use crate::configs::settings::SETTINGS;
use crate::domain::models::{BankAccount, BankAccountView, Ledger, LedgerView};
use crate::event_sourcing::command::BankAccountCommand;
use crate::interest::repository::{InterestRepository, PgInterestRepository};
use crate::repository::adapter::{Adapter, DatabaseClient};
use crate::repository::configs::{configure_bank_account, configure_ledger};
use crate::repository::pools::DbPools;
use crate::SharedState;

use postgres_es::{PostgresCqrs, PostgresViewRepository};

/// Build a `DbPools` from the current `SETTINGS`.
async fn create_db_pools(max_write: u32, min_write: u32, max_read: u32, min_read: u32) -> DbPools {
    let primary: PgPool = PgPoolOptions::new()
        .max_connections(max_write)
        .min_connections(min_write)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(600))
        .connect(&SETTINGS.database.connection_string())
        .await
        .expect("Failed to connect to primary database");

    let replica = match SETTINGS.database.replica_connection_string() {
        Some(url) => {
            let pool = PgPoolOptions::new()
                .max_connections(max_read)
                .min_connections(min_read)
                .acquire_timeout(Duration::from_secs(3))
                .idle_timeout(Duration::from_secs(600))
                .connect(&url)
                .await
                .expect("Failed to connect to read replica database");
            info!("Read replica pool created");
            Some(pool)
        }
        None => {
            info!("No read replica configured, using primary for all reads");
            None
        }
    };

    DbPools::new(primary, replica)
}

/// Create application state for the worker binary.
/// Skips mpsc command channel and BankAccount CQRS (worker never handles HTTP commands).
/// Includes: database, cache, ledger CQRS, interest_repo, fx_rate_service.
pub async fn new_worker_state() -> SharedState {
    let pools = create_db_pools(15, 2, 20, 2).await;

    let (ledger_cqrs, ledger_query) = configure_ledger(&pools);
    let ledger_loader_saver = LedgerLoaderSaver {
        cqrs: ledger_cqrs,
        query: ledger_query,
    };

    let cache = match redis::Client::open(SETTINGS.redis.connection_string()) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to Redis: {:?}", e);
            panic!("Redis connection required for startup");
        }
    };

    // Initialize FX rate service with providers
    let fx_rate_service = {
        use crate::common::fx_rate::{CoinGeckoProvider, ExchangeRateProvider, FxRateProvider};
        let providers: Vec<Box<dyn FxRateProvider>> = vec![
            Box::new(CoinGeckoProvider::new()),
            Box::new(ExchangeRateProvider::new()),
        ];
        Arc::new(FxRateService::new(providers, Arc::new(cache.clone())))
    };

    // Load assets from DB
    let adapter = Adapter::new(pools.clone());
    let asset_registry = match adapter.load_assets().await {
        Ok(assets) => {
            info!("Loaded {} assets from database", assets.len());
            AssetRegistry::new(assets)
        }
        Err(e) => {
            error!("Failed to load assets from DB, using defaults: {:?}", e);
            AssetRegistry::with_defaults()
        }
    };

    let interest_repo: Arc<dyn InterestRepository> = Arc::new(PgInterestRepository::new(
        pools.read().clone(),
        pools.write().clone(),
    ));

    Arc::new(
        ApplicationState::<DbPools>::new(Adapter::new(pools))
            .with_cache(cache)
            .with_ledger(ledger_loader_saver)
            .with_asset_registry(asset_registry)
            .with_fx_rate_service(fx_rate_service)
            .with_interest_repository(interest_repo),
    )
}

#[derive(Clone)]
pub struct ApplicationState<C: DatabaseClient + Send + Sync> {
    pub bank_account: Option<BankAccountLoaderSaver>,
    pub ledger: Option<LedgerLoaderSaver>,
    pub database: Arc<Adapter<C>>,
    pub cache: Option<Arc<redis::Client>>,
    pub command_sender: Option<Arc<Sender<BankAccountCommand>>>,
    pub asset_registry: AssetRegistry,
    pub fx_rate_service: Option<Arc<FxRateService>>,
    pub interest_repo: Option<Arc<dyn InterestRepository>>,
}

impl<C: DatabaseClient + Send + Sync> ApplicationState<C> {
    pub fn new(database: Adapter<C>) -> Self {
        Self {
            bank_account: None,
            ledger: None,
            database: Arc::new(database),
            cache: None,
            command_sender: None,
            asset_registry: AssetRegistry::with_defaults(),
            fx_rate_service: None,
            interest_repo: None,
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

    pub fn with_command_sender(mut self, sender: Sender<BankAccountCommand>) -> Self {
        self.command_sender = Some(Arc::new(sender));
        self
    }

    pub fn with_asset_registry(mut self, registry: AssetRegistry) -> Self {
        self.asset_registry = registry;
        self
    }

    pub fn with_fx_rate_service(mut self, service: Arc<FxRateService>) -> Self {
        self.fx_rate_service = Some(service);
        self
    }

    pub fn with_interest_repository(mut self, repo: Arc<dyn InterestRepository>) -> Self {
        self.interest_repo = Some(repo);
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

pub async fn new_application_state(tx: Sender<BankAccountCommand>) -> SharedState {
    // Primary: max=30, min=3; Replica: max=40, min=5
    let pools = create_db_pools(30, 3, 40, 5).await;

    let (ledger_cqrs, ledger_query) = configure_ledger(&pools);
    let ledger_loader_saver = LedgerLoaderSaver {
        cqrs: ledger_cqrs,
        query: ledger_query,
    };

    let cache = match redis::Client::open(SETTINGS.redis.connection_string()) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to Redis: {:?}", e);
            panic!("Redis connection required for startup");
        }
    };

    // Initialize FX rate service with providers
    let fx_rate_service = {
        use crate::common::fx_rate::{CoinGeckoProvider, ExchangeRateProvider, FxRateProvider};
        let providers: Vec<Box<dyn FxRateProvider>> = vec![
            Box::new(CoinGeckoProvider::new()),
            Box::new(ExchangeRateProvider::new()),
        ];
        Arc::new(FxRateService::new(providers, Arc::new(cache.clone())))
    };

    let (bc_cqrs, bc_query) = configure_bank_account(
        &pools,
        ledger_loader_saver.clone(),
        Some(fx_rate_service.clone()),
    );

    // Load assets from DB
    let adapter = Adapter::new(pools.clone());
    let asset_registry = match adapter.load_assets().await {
        Ok(assets) => {
            info!("Loaded {} assets from database", assets.len());
            AssetRegistry::new(assets)
        }
        Err(e) => {
            error!("Failed to load assets from DB, using defaults: {:?}", e);
            AssetRegistry::with_defaults()
        }
    };

    let interest_repo: Arc<dyn InterestRepository> = Arc::new(PgInterestRepository::new(
        pools.read().clone(),
        pools.write().clone(),
    ));

    Arc::new(
        ApplicationState::<DbPools>::new(Adapter::new(pools))
            .with_cache(cache)
            .with_bank_account(BankAccountLoaderSaver {
                cqrs: bc_cqrs,
                query: bc_query,
            })
            .with_ledger(ledger_loader_saver)
            .with_command_sender(tx)
            .with_asset_registry(asset_registry)
            .with_fx_rate_service(fx_rate_service)
            .with_interest_repository(interest_repo),
    )
}
