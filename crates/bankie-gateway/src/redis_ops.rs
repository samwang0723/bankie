use redis::AsyncCommands;
use tracing::error;

/// Get a string value from Redis by key.
pub async fn get_value(
    client: &redis::Client,
    key: &str,
) -> Result<Option<String>, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let result: Option<String> = con.get(key).await?;
    Ok(result)
}

/// Set a key with a value and expiry in seconds.
pub async fn set_ex(
    client: &redis::Client,
    key: &str,
    value: &str,
    ttl_seconds: i64,
) -> Result<(), redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    con.set_ex::<_, _, ()>(key, value, ttl_seconds as u64)
        .await?;
    Ok(())
}

/// Get the remaining TTL of a key in seconds.
/// Returns -2 if the key does not exist, -1 if no expiry is set.
pub async fn get_ttl(client: &redis::Client, key: &str) -> Result<i64, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let ttl: i64 = redis::cmd("TTL").arg(key).query_async(&mut con).await?;
    Ok(ttl)
}

/// Delete a key from Redis. Returns the number of keys removed.
pub async fn del_key(client: &redis::Client, key: &str) -> Result<i64, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let removed: i64 = con.del(key).await?;
    Ok(removed)
}

/// Increment a key and set expiry if it's the first increment. Returns the new count.
pub async fn incr_with_expiry(
    client: &redis::Client,
    key: &str,
    ttl_seconds: i64,
) -> Result<i64, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;
    let count: i64 = con.incr(key, 1i64).await?;
    if count == 1 {
        // First increment — set expiry
        let _: () = con.expire(key, ttl_seconds).await?;
    }
    Ok(count)
}

/// Execute a Lua script atomically for token bucket rate limiting.
/// Returns (allowed: bool, remaining: i64, reset_at: i64).
pub async fn rate_limit_check(
    client: &redis::Client,
    key: &str,
    burst: i64,
    sustained_per_min: i64,
    now_epoch_secs: i64,
) -> Result<RateLimitResult, redis::RedisError> {
    let mut con = client.get_multiplexed_async_connection().await?;

    // Token bucket Lua script:
    // - Refills tokens based on elapsed time since last check
    // - Attempts to consume one token
    // - Returns: allowed (1/0), remaining tokens, window reset epoch
    let script = redis::Script::new(
        r#"
        local key = KEYS[1]
        local burst = tonumber(ARGV[1])
        local rate_per_sec = tonumber(ARGV[2]) / 60.0
        local now = tonumber(ARGV[3])
        local window_secs = 60

        local data = redis.call('HMGET', key, 'tokens', 'last_refill')
        local tokens = tonumber(data[1])
        local last_refill = tonumber(data[2])

        if tokens == nil then
            -- First request: initialize bucket
            tokens = burst - 1
            redis.call('HMSET', key, 'tokens', tokens, 'last_refill', now)
            redis.call('EXPIRE', key, window_secs * 2)
            return {1, tokens, now + window_secs}
        end

        -- Refill tokens based on elapsed time
        local elapsed = now - last_refill
        local refill = math.floor(elapsed * rate_per_sec)
        if refill > 0 then
            tokens = math.min(burst, tokens + refill)
            last_refill = now
        end

        -- Try to consume a token
        if tokens > 0 then
            tokens = tokens - 1
            redis.call('HMSET', key, 'tokens', tokens, 'last_refill', last_refill)
            redis.call('EXPIRE', key, window_secs * 2)
            return {1, tokens, now + window_secs}
        else
            redis.call('HMSET', key, 'tokens', tokens, 'last_refill', last_refill)
            redis.call('EXPIRE', key, window_secs * 2)
            return {0, 0, now + window_secs}
        end
        "#,
    );

    let result: (i64, i64, i64) = script
        .key(key)
        .arg(burst)
        .arg(sustained_per_min)
        .arg(now_epoch_secs)
        .invoke_async(&mut con)
        .await
        .map_err(|e| {
            error!("Rate limit Lua script error: {}", e);
            e
        })?;

    Ok(RateLimitResult {
        allowed: result.0 == 1,
        remaining: result.1,
        reset_at: result.2,
    })
}

/// Result from the token bucket rate limit check.
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub remaining: i64,
    pub reset_at: i64,
}
