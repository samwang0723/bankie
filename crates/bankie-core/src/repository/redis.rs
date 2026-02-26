use redis::AsyncCommands;
use tracing::{error, info};
use uuid::Uuid;

pub const LOCK_KEY: &str = "outbox_lock";
pub const LOCK_TIMEOUT: i64 = 10 * 60; // seconds

pub async fn acquire_lock(
    client: &redis::Client,
    lock_key: &str,
    lock_timeout: i64,
) -> Option<String> {
    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get Redis connection for lock acquire: {:?}", e);
            return None;
        }
    };
    let lock_value = Uuid::new_v4().to_string();
    let result: bool = match con.set_nx(lock_key, &lock_value).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to SET NX on Redis: {:?}", e);
            return None;
        }
    };

    if result {
        if let Err(e) = con.expire::<_, ()>(lock_key, lock_timeout).await {
            error!("Failed to set lock expiry: {:?}", e);
        }
        info!("Acquired lock with value: {}", lock_value);
        Some(lock_value)
    } else {
        None
    }
}

pub async fn release_lock(client: &redis::Client, lock_key: &str, lock_value: &str) {
    let mut con = match client.get_multiplexed_async_connection().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get Redis connection for lock release: {:?}", e);
            return;
        }
    };
    let current_lock_value: String = match con.get(lock_key).await {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to GET lock value: {:?}", e);
            return;
        }
    };
    if current_lock_value == lock_value {
        if let Err(e) = con.del::<_, ()>(lock_key).await {
            error!("Failed to DEL lock: {:?}", e);
        } else {
            info!("Released lock with value: {}", lock_value);
        }
    }
}

/// Set a key with NX (only if not exists) and EX (expiry in seconds).
/// Used for idempotency tokens.
pub async fn set_nx_ex(
    client: &redis::Client,
    key: &str,
    value: &str,
    ttl_seconds: i64,
) -> Result<bool, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let result: bool = redis::cmd("SET")
        .arg(key)
        .arg(value)
        .arg("NX")
        .arg("EX")
        .arg(ttl_seconds)
        .query_async(&mut con)
        .await?;
    Ok(result)
}

/// Get a value by key from Redis.
pub async fn get_value(
    client: &redis::Client,
    key: &str,
) -> Result<Option<String>, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let result: Option<String> = con.get(key).await?;
    Ok(result)
}
