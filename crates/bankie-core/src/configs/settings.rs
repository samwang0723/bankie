use config::Config;
use lazy_static::lazy_static;
use serde::Deserialize;
use tracing::info;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSettings {
    pub host: String,
    pub port: String,
    pub user: String,
    pub dbname: String,
    #[serde(skip_deserializing)]
    pub dbpasswd: String,
    pub read_replica: Option<ReadReplicaSettings>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadReplicaSettings {
    pub host: String,
    pub port: String,
    pub user: Option<String>,
    pub dbname: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisSettings {
    pub host: String,
    pub port: String,
    #[serde(default)]
    pub password: String,
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
        let filename = format!("config.{}.yaml", env);
        // Try current directory first, then workspace root (for running from crate subdirs)
        let file = if std::path::Path::new(&filename).exists() {
            filename.clone()
        } else {
            // Walk up to find the config file (supports workspace layout)
            let mut dir = std::env::current_dir().unwrap_or_default();
            loop {
                let candidate = dir.join(&filename);
                if candidate.exists() {
                    break candidate.to_string_lossy().to_string();
                }
                if !dir.pop() {
                    break filename.clone();
                }
            }
        };
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
        if let Ok(redis_password) = std::env::var("REDIS_PASSWORD") {
            cfg.redis.password = redis_password;
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

    /// Build a connection string for the read replica, falling back to
    /// primary settings for user/dbname/password when not specified.
    pub fn replica_connection_string(&self) -> Option<String> {
        self.read_replica.as_ref().map(|r| {
            let user = r.user.as_deref().unwrap_or(&self.user);
            let dbname = r.dbname.as_deref().unwrap_or(&self.dbname);
            format!(
                "postgres://{}:{}@{}:{}/{}",
                user, self.dbpasswd, r.host, r.port, dbname
            )
        })
    }
}

impl RedisSettings {
    pub fn connection_string(&self) -> String {
        if self.password.is_empty() {
            format!("redis://{}:{}", self.host, self.port)
        } else {
            format!("redis://:{}@{}:{}", self.password, self.host, self.port)
        }
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
        assert_eq!(settings.redis.host, "localhost");
        assert_eq!(settings.redis.port, "6379");
    }

    #[test]
    fn test_customize_from_env() {
        env::set_var("DB_PASSWD", "test_password");

        let settings = Settings::new();
        assert_eq!(settings.database.dbpasswd, "test_password");
    }

    #[test]
    fn test_database_connection_string() {
        let db_settings = DatabaseSettings {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            user: "test_user".to_string(),
            dbname: "test_db".to_string(),
            dbpasswd: "test_password".to_string(),
            read_replica: None,
        };

        let connection_string = db_settings.connection_string();
        assert_eq!(
            connection_string,
            "postgres://test_user:test_password@localhost:5432/test_db"
        );
    }

    #[test]
    fn test_replica_connection_string_none() {
        let db_settings = DatabaseSettings {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            user: "test_user".to_string(),
            dbname: "test_db".to_string(),
            dbpasswd: "test_password".to_string(),
            read_replica: None,
        };
        assert!(db_settings.replica_connection_string().is_none());
    }

    #[test]
    fn test_replica_connection_string_with_defaults() {
        let db_settings = DatabaseSettings {
            host: "primary".to_string(),
            port: "5432".to_string(),
            user: "app_user".to_string(),
            dbname: "app_db".to_string(),
            dbpasswd: "secret".to_string(),
            read_replica: Some(ReadReplicaSettings {
                host: "replica-host".to_string(),
                port: "5433".to_string(),
                user: None,
                dbname: None,
            }),
        };
        assert_eq!(
            db_settings.replica_connection_string().unwrap(),
            "postgres://app_user:secret@replica-host:5433/app_db"
        );
    }

    #[test]
    fn test_replica_connection_string_with_overrides() {
        let db_settings = DatabaseSettings {
            host: "primary".to_string(),
            port: "5432".to_string(),
            user: "app_user".to_string(),
            dbname: "app_db".to_string(),
            dbpasswd: "secret".to_string(),
            read_replica: Some(ReadReplicaSettings {
                host: "replica-host".to_string(),
                port: "5433".to_string(),
                user: Some("ro_user".to_string()),
                dbname: Some("ro_db".to_string()),
            }),
        };
        assert_eq!(
            db_settings.replica_connection_string().unwrap(),
            "postgres://ro_user:secret@replica-host:5433/ro_db"
        );
    }

    #[test]
    fn test_redis_connection_string() {
        let redis_settings = RedisSettings {
            host: "localhost".to_string(),
            port: "6379".to_string(),
            password: String::new(),
        };
        assert_eq!(redis_settings.connection_string(), "redis://localhost:6379");

        let redis_settings_auth = RedisSettings {
            host: "localhost".to_string(),
            port: "6379".to_string(),
            password: "secret".to_string(),
        };
        assert_eq!(
            redis_settings_auth.connection_string(),
            "redis://:secret@localhost:6379"
        );
    }
}
