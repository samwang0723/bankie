use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::path::Path;

#[allow(unused)]
#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    // Create a connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgres://APP_NAME_UND_app:DB_PASSWORD@localhost:5432/APP_NAME_UND_main")
        .await?;

    // Specify the path to the migrations directory
    let migrator = Migrator::new(Path::new("./db/migrations")).await?;

    // Run the migrations
    migrator.run(&pool).await?;

    Ok(())
}
