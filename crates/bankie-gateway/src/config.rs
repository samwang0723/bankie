use config::Config;
use lazy_static::lazy_static;
use serde::Deserialize;
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

    #[test]
    fn test_database_connection_string() {
        let db = DatabaseSettings {
            host: "localhost".to_string(),
            port: "5432".to_string(),
            user: "gw_user".to_string(),
            dbname: "bankie_main".to_string(),
            dbpasswd: "secret".to_string(),
        };
        assert_eq!(
            db.connection_string(),
            "postgres://gw_user:secret@localhost:5432/bankie_main"
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
