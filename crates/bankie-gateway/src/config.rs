use config::Config;
use lazy_static::lazy_static;
use serde::Deserialize;
use sqlx::PgPool;
use tracing::info;

/// Gateway-specific configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewaySettings {
    pub database: DatabaseSettings,
    pub redis: RedisSettings,
    #[serde(default = "default_core_url")]
    pub core_url: String,
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(skip_deserializing)]
    pub jwt_secret: String,
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

fn default_core_url() -> String {
    "http://localhost:3030".to_string()
}

fn default_listen_addr() -> String {
    "0.0.0.0:4040".to_string()
}

lazy_static! {
    pub static ref SETTINGS: GatewaySettings = GatewaySettings::new();
}

impl GatewaySettings {
    pub fn new() -> Self {
        let env = std::env::var("ENV").unwrap_or_else(|_| "local".to_string());
        let mut cfg = Self::load_from_file(&env);
        Self::customize_from_env(&mut cfg);
        cfg
    }

    fn load_from_file(env: &str) -> Self {
        // Try gateway-specific config first, then shared config
        let gateway_filename = format!("config.gateway.{}.yaml", env);
        let shared_filename = format!("config.{}.yaml", env);

        let file = Self::find_config_file(&gateway_filename)
            .or_else(|| Self::find_config_file(&shared_filename))
            .unwrap_or(shared_filename);

        info!("Loading gateway configuration from: {}", file);
        let settings = Config::builder()
            .add_source(config::File::with_name(&file))
            .build()
            .expect("Failed to build gateway configuration");

        settings
            .try_deserialize()
            .expect("Failed to deserialize gateway configuration")
    }

    fn find_config_file(filename: &str) -> Option<String> {
        if std::path::Path::new(filename).exists() {
            return Some(filename.to_string());
        }
        // Walk up to find the config file (supports workspace layout)
        let mut dir = std::env::current_dir().unwrap_or_default();
        loop {
            let candidate = dir.join(filename);
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    fn customize_from_env(cfg: &mut Self) {
        if let Ok(db_password) = std::env::var("DB_PASSWD") {
            cfg.database.dbpasswd = db_password;
        }
        if let Ok(core_url) = std::env::var("CORE_URL") {
            cfg.core_url = core_url;
        }
        if let Ok(addr) = std::env::var("GATEWAY_LISTEN_ADDR") {
            cfg.listen_addr = addr;
        }
        if let Ok(secret) = std::env::var("JWT_SECRET") {
            cfg.jwt_secret = secret;
        }
        if let Ok(redis_password) = std::env::var("REDIS_PASSWORD") {
            cfg.redis.password = redis_password;
        }
    }
}

impl Default for GatewaySettings {
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

impl ReadReplicaSettings {
    pub fn connection_string(&self, primary: &DatabaseSettings) -> String {
        let user = self.user.as_deref().unwrap_or(&primary.user);
        let dbname = self.dbname.as_deref().unwrap_or(&primary.dbname);
        format!(
            "postgres://{}:{}@{}:{}/{}",
            user, primary.dbpasswd, self.host, self.port, dbname
        )
    }
}

/// Holds primary (write) and replica (read) database connection pools.
/// If no replica is configured, both point to the same primary pool.
#[derive(Clone)]
pub struct DbPools {
    primary: PgPool,
    replica: PgPool,
}

impl DbPools {
    pub fn new(primary: PgPool, replica: Option<PgPool>) -> Self {
        let replica = replica.unwrap_or_else(|| primary.clone());
        Self { primary, replica }
    }

    /// Read pool — for queries that tolerate replication lag.
    pub fn read(&self) -> &PgPool {
        &self.replica
    }

    /// Write pool — for mutations and lag-sensitive reads.
    pub fn write(&self) -> &PgPool {
        &self.primary
    }

    /// Create a dummy `DbPools` for unit tests that use mock repositories.
    /// The pools are not connected to any database.
    #[cfg(test)]
    pub fn test_dummy() -> Self {
        let opts = sqlx::postgres::PgConnectOptions::new()
            .host("localhost")
            .port(5432)
            .username("test")
            .database("test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy_with(opts);
        Self {
            primary: pool.clone(),
            replica: pool,
        }
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

    fn test_db_settings() -> DatabaseSettings {
        DatabaseSettings {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            user: "gw_user".to_string(),
            dbname: "bankie_main".to_string(),
            dbpasswd: "secret".to_string(),
            read_replica: None,
        }
    }

    #[test]
    fn test_database_connection_string() {
        let db = test_db_settings();
        assert_eq!(
            db.connection_string(),
            "postgres://gw_user:secret@localhost:5432/bankie_main"
        );
    }

    #[test]
    fn test_replica_connection_string_with_defaults() {
        let mut db = test_db_settings();
        db.read_replica = Some(ReadReplicaSettings {
            host: "replica-host".to_string(),
            port: "5433".to_string(),
            user: None,
            dbname: None,
        });
        assert_eq!(
            db.replica_connection_string().unwrap(),
            "postgres://gw_user:secret@replica-host:5433/bankie_main"
        );
    }

    #[test]
    fn test_replica_connection_string_with_overrides() {
        let mut db = test_db_settings();
        db.read_replica = Some(ReadReplicaSettings {
            host: "replica-host".to_string(),
            port: "5433".to_string(),
            user: Some("replica_user".to_string()),
            dbname: Some("replica_db".to_string()),
        });
        assert_eq!(
            db.replica_connection_string().unwrap(),
            "postgres://replica_user:secret@replica-host:5433/replica_db"
        );
    }

    #[test]
    fn test_replica_connection_string_none_when_no_replica() {
        let db = test_db_settings();
        assert!(db.replica_connection_string().is_none());
    }

    #[test]
    fn test_read_replica_settings_connection_string() {
        let primary = test_db_settings();
        let replica = ReadReplicaSettings {
            host: "replica-host".to_string(),
            port: "5433".to_string(),
            user: None,
            dbname: None,
        };
        assert_eq!(
            replica.connection_string(&primary),
            "postgres://gw_user:secret@replica-host:5433/bankie_main"
        );
    }

    #[test]
    fn test_redis_connection_string() {
        let redis = RedisSettings {
            host: "localhost".to_string(),
            port: "6379".to_string(),
            password: String::new(),
        };
        assert_eq!(redis.connection_string(), "redis://localhost:6379");
    }

    #[test]
    fn test_redis_connection_string_with_password() {
        let redis = RedisSettings {
            host: "redis-host".to_string(),
            port: "6379".to_string(),
            password: "secret123".to_string(),
        };
        assert_eq!(
            redis.connection_string(),
            "redis://:secret123@redis-host:6379"
        );
    }

    #[test]
    fn test_default_core_url() {
        assert_eq!(default_core_url(), "http://localhost:3030");
    }

    #[test]
    fn test_default_listen_addr() {
        assert_eq!(default_listen_addr(), "0.0.0.0:4040");
    }
}
