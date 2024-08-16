use config::Config;
use lazy_static::lazy_static;
use serde::Deserialize;
use tracing::info;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub database: DatabaseSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    pub host: String,
    pub port: String,
    pub user: String,
    pub dbname: String,
    #[serde(skip_deserializing)]
    pub dbpasswd: String,
}

lazy_static! {
    pub static ref SETTINGS: Settings = Settings::new();
}

impl Settings {
    pub fn new() -> Self {
        let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
        let mut cfg = Self::load_from_file(&env);
        Self::customize_from_env(&mut cfg);

        cfg
    }

    fn load_from_file(env: &str) -> Self {
        let file = format!("config.{}.yaml", env);
        info!("Loading configuration from: {}", file);
        let settings = Config::builder()
            .add_source(config::File::with_name(&file))
            .build()
            .expect("Failed to build configuration");

        settings
            .try_deserialize()
            .expect("Failed to deserialize configuration")
    }

    fn customize_from_env(cfg: &mut Self) {
        if let Ok(db_password) = std::env::var("DB_PASSWD") {
            cfg.database.dbpasswd = db_password;
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl DatabaseSettings {
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.dbpasswd, self.host, self.port, self.dbname
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_load_from_file() {
        let settings = Settings::new();
        assert_eq!(settings.database.host, "localhost");
        assert_eq!(settings.database.port, "5432");
        assert_eq!(settings.database.user, "bankie_app");
        assert_eq!(settings.database.dbname, "bankie_main");
    }

    #[test]
    fn test_customize_from_env() {
        env::set_var("DB_PASSWD", "test_password");

        let settings = Settings::new();
        assert_eq!(settings.database.dbpasswd, "test_password");
    }

    #[test]
    fn test_connection_string() {
        let db_settings = DatabaseSettings {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            user: "test_user".to_string(),
            dbname: "test_db".to_string(),
            dbpasswd: "test_password".to_string(),
        };

        let connection_string = db_settings.connection_string();
        assert_eq!(
            connection_string,
            "postgres://test_user:test_password@localhost:5432/test_db"
        );
    }
}
