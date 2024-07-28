use crate::configs::settings::SETTINGS;
use postgres_es::{default_postgress_pool, PostgresEventRepository};

async fn configure_repo() -> PostgresEventRepository {
    let pool = default_postgress_pool(&SETTINGS.database.connection_string()).await;
    PostgresEventRepository::new(pool)
}
