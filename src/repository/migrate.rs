use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use std::path::Path;

#[allow(dead_code)]
#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    dotenv::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        let user = std::env::var("DB_USER").unwrap_or_else(|_| "bankie_app".to_string());
        let password = std::env::var("DB_PASSWD").unwrap_or_default();
        let host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
        let dbname = std::env::var("DB_NAME").unwrap_or_else(|_| "bankie_main".to_string());
        format!(
            "postgres://{}:{}@{}:{}/{}",
            user, password, host, port, dbname
        )
    });

    // Create a connection pool
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Specify the path to the migrations directory
    let migrator = Migrator::new(Path::new("./db/migrations")).await?;

    // Run the migrations
    migrator.run(&pool).await?;

    println!("Migrations completed successfully.");

    Ok(())
}
