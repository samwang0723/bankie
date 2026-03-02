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
use crate::repository::adapter::{Adapter, DatabaseClient};
use crate::repository::configs::{configure_bank_account, configure_ledger};
use crate::SharedState;

use postgres_es::{PostgresCqrs, PostgresViewRepository};

#[derive(Clone)]
pub struct ApplicationState<C: DatabaseClient + Send + Sync> {
    pub bank_account: Option<BankAccountLoaderSaver>,
    pub ledger: Option<LedgerLoaderSaver>,
    pub database: Arc<Adapter<C>>,
    pub cache: Option<Arc<redis::Client>>,
    pub command_sender: Option<Arc<Sender<BankAccountCommand>>>,
    pub asset_registry: AssetRegistry,
    pub fx_rate_service: Option<Arc<FxRateService>>,
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
    // H3 FIX: Explicit connection pool sizing instead of library defaults
    let pool: PgPool = PgPoolOptions::new()
        .max_connections(50)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(600))
        .connect(&SETTINGS.database.connection_string())
        .await
        .expect("Failed to connect to database");

    let (ledger_cqrs, ledger_query) = configure_ledger(pool.clone());
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
        pool.clone(),
        ledger_loader_saver.clone(),
        Some(fx_rate_service.clone()),
    );

    // Load assets from DB
    let adapter = Adapter::new(pool.clone());
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

    Arc::new(
        ApplicationState::<PgPool>::new(Adapter::new(pool))
            .with_cache(cache)
            .with_bank_account(BankAccountLoaderSaver {
                cqrs: bc_cqrs,
                query: bc_query,
            })
            .with_ledger(ledger_loader_saver)
            .with_command_sender(tx)
            .with_asset_registry(asset_registry)
            .with_fx_rate_service(fx_rate_service),
    )
}
