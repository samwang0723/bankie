use redis::AsyncCommands;
use tracing::info;
use uuid::Uuid;

pub const LOCK_KEY: &str = "outbox_lock";
pub const LOCK_TIMEOUT: i64 = 10 * 60; // seconds

pub async fn acquire_lock(
    client: &redis::Client,
    lock_key: &str,
    lock_timeout: i64,
) -> Option<String> {
    let mut con = client.get_multiplexed_async_connection().await.unwrap();
    let lock_value = Uuid::new_v4().to_string();
    let result: bool = con.set_nx(lock_key, &lock_value).await.unwrap();

    if result {
        let _: () = con.expire(lock_key, lock_timeout).await.unwrap();
        info!("Acquired lock with value: {}", lock_value);
        Some(lock_value)
    } else {
        None
    }
}

pub async fn release_lock(client: &redis::Client, lock_key: &str, lock_value: &str) {
    let mut con = client.get_multiplexed_async_connection().await.unwrap();
    let current_lock_value: String = con.get(lock_key).await.unwrap();
    if current_lock_value == lock_value {
        let _: () = con.del(lock_key).await.unwrap();
        info!("Released lock with value: {}", lock_value);
    }
}
