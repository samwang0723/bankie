use config::Config;
use lazy_static::lazy_static;
use serde::Deserialize;

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

        println!("{:?}", cfg);
        cfg
    }

    fn load_from_file(env: &str) -> Self {
        let file = format!("config.{}.yaml", env);
        println!("Loading configuration from: {}", file);
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

impl DatabaseSettings {
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.dbpasswd, self.host, self.port, self.dbname
        )
    }
}
