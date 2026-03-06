use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Exchange rate snapshot at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxRate {
    pub from_currency: String,
    pub to_currency: String,
    pub rate: Decimal,
    pub source: FxRateSource,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Source of an FX rate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FxRateSource {
    Static,
    CoinGecko,
    ExchangeRateApi,
    Backfill,
    Mock,
}

impl std::fmt::Display for FxRateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Static => write!(f, "static"),
            Self::CoinGecko => write!(f, "coingecko"),
            Self::ExchangeRateApi => write!(f, "exchangerate-api"),
            Self::Backfill => write!(f, "backfill"),
            Self::Mock => write!(f, "mock"),
        }
    }
}

/// Result of converting an amount to USD.
#[derive(Debug, Clone)]
pub struct UsdConversion {
    pub fx_rate_to_usd: Decimal,
    pub amount_usd: Decimal,
    pub source: FxRateSource,
}

/// Trait for fetching exchange rates from external sources.
#[async_trait]
pub trait FxRateProvider: Send + Sync {
    /// Get the USD exchange rate for a given currency.
    /// Returns: 1 unit of `currency` = X USD.
    async fn get_usd_rate(&self, currency: &str) -> Result<FxRate, anyhow::Error>;

    /// Which currencies this provider handles.
    fn supported_currencies(&self) -> &[&str];
}

/// FxRateService coordinates rate lookups across providers with Redis caching.
pub struct FxRateService {
    providers: Vec<Box<dyn FxRateProvider>>,
    redis: Arc<redis::Client>,
}

/// TTL for cached crypto rates (volatile).
const CRYPTO_CACHE_TTL_SECS: i64 = 60;
/// TTL for cached fiat rates (stable).
const FIAT_CACHE_TTL_SECS: i64 = 3600;

/// Crypto currencies for TTL classification.
const CRYPTO_CURRENCIES: &[&str] = &["BTC", "ETH", "USDT"];

impl FxRateService {
    pub fn new(providers: Vec<Box<dyn FxRateProvider>>, redis: Arc<redis::Client>) -> Self {
        Self { providers, redis }
    }

    /// Convert an amount to USD using cached or live rates.
    /// Returns None if rate is unavailable (graceful degradation).
    pub async fn convert_to_usd(&self, amount: Decimal, currency: &str) -> Option<UsdConversion> {
        // 1. Static: USD → USD is always 1.0
        if currency == "USD" {
            return Some(UsdConversion {
                fx_rate_to_usd: Decimal::ONE,
                amount_usd: amount.round_dp(2),
                source: FxRateSource::Static,
            });
        }

        // 2. Check Redis cache
        if let Some(cached) = self.get_cached_rate(currency).await {
            let amount_usd = (amount * cached.rate).round_dp(2);
            return Some(UsdConversion {
                fx_rate_to_usd: cached.rate,
                amount_usd,
                source: cached.source,
            });
        }

        // 3. Fetch from provider
        for provider in &self.providers {
            if provider.supported_currencies().contains(&currency) {
                match provider.get_usd_rate(currency).await {
                    Ok(rate) => {
                        self.cache_rate(&rate).await;
                        let amount_usd = (amount * rate.rate).round_dp(2);
                        return Some(UsdConversion {
                            fx_rate_to_usd: rate.rate,
                            amount_usd,
                            source: rate.source,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(currency, error = %e, "FX rate fetch failed");
                        continue;
                    }
                }
            }
        }

        // 4. All providers failed
        tracing::error!(
            currency,
            "No FX rate available, transaction proceeds without USD amount"
        );
        None
    }

    async fn get_cached_rate(&self, currency: &str) -> Option<FxRate> {
        let key = format!("fx:USD:{}", currency);
        let mut con = match self.redis.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Redis connection error for FX cache: {}", e);
                return None;
            }
        };
        let value: Option<String> = match redis::AsyncCommands::get(&mut con, &key).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Redis GET error for FX cache: {}", e);
                return None;
            }
        };
        value.and_then(|v| serde_json::from_str(&v).ok())
    }

    async fn cache_rate(&self, rate: &FxRate) {
        let key = format!("fx:USD:{}", rate.from_currency);
        let ttl = if CRYPTO_CURRENCIES.contains(&rate.from_currency.as_str()) {
            CRYPTO_CACHE_TTL_SECS
        } else {
            FIAT_CACHE_TTL_SECS
        };
        let value = match serde_json::to_string(rate) {
            Ok(v) => v,
            Err(_) => return,
        };
        let mut con = match self.redis.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Redis connection error caching FX rate: {}", e);
                return;
            }
        };
        let result: Result<(), redis::RedisError> =
            redis::AsyncCommands::set_ex(&mut con, &key, &value, ttl as u64).await;
        if let Err(e) = result {
            tracing::warn!("Redis SET error caching FX rate: {}", e);
        }
    }
}

// --- Provider Implementations ---

/// CoinGecko provider for crypto rates (BTC, ETH, USDT).
pub struct CoinGeckoProvider {
    client: reqwest::Client,
}

impl Default for CoinGeckoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CoinGeckoProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("bankie/1.0")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    fn coin_id(currency: &str) -> Option<&'static str> {
        match currency {
            "BTC" => Some("bitcoin"),
            "ETH" => Some("ethereum"),
            "USDT" => Some("tether"),
            _ => None,
        }
    }
}

#[async_trait]
impl FxRateProvider for CoinGeckoProvider {
    async fn get_usd_rate(&self, currency: &str) -> Result<FxRate, anyhow::Error> {
        let coin_id =
            Self::coin_id(currency).ok_or_else(|| anyhow::anyhow!("Unsupported: {}", currency))?;

        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            coin_id
        );
        let response = self.client.get(&url).send().await?;
        let status = response.status();
        let body = response.text().await?;
        tracing::debug!(
            coin_id,
            %status,
            %body,
            "CoinGecko raw response"
        );
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "CoinGecko returned HTTP {}: {}",
                status,
                body
            ));
        }
        let resp: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("CoinGecko JSON parse error: {} body={}", e, body))?;
        let rate = resp
            .get(coin_id)
            .and_then(|v| v.get("usd"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing rate for {} in response: {}", coin_id, body))?;

        let rate_decimal =
            Decimal::try_from(rate).map_err(|e| anyhow::anyhow!("Invalid rate value: {}", e))?;
        if rate_decimal <= Decimal::ZERO {
            return Err(anyhow::anyhow!("Invalid rate (zero or negative): {}", rate));
        }

        Ok(FxRate {
            from_currency: currency.to_string(),
            to_currency: "USD".to_string(),
            rate: rate_decimal,
            source: FxRateSource::CoinGecko,
            timestamp: chrono::Utc::now(),
        })
    }

    fn supported_currencies(&self) -> &[&str] {
        &["BTC", "ETH", "USDT"]
    }
}

/// ExchangeRate API provider for fiat rates (TWD).
pub struct ExchangeRateProvider {
    client: reqwest::Client,
}

impl Default for ExchangeRateProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ExchangeRateProvider {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

#[async_trait]
impl FxRateProvider for ExchangeRateProvider {
    async fn get_usd_rate(&self, currency: &str) -> Result<FxRate, anyhow::Error> {
        let url = "https://open.er-api.com/v6/latest/USD";
        let resp: serde_json::Value = self.client.get(url).send().await?.json().await?;
        let usd_to_currency = resp
            .get("rates")
            .and_then(|r| r.get(currency))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing rate for {}", currency))?;

        if usd_to_currency <= 0.0 {
            return Err(anyhow::anyhow!(
                "Invalid rate (zero or negative): {}",
                usd_to_currency
            ));
        }

        // API returns 1 USD = X {currency}, we need 1 {currency} = Y USD
        let rate = Decimal::ONE
            / Decimal::try_from(usd_to_currency)
                .map_err(|e| anyhow::anyhow!("Invalid rate value: {}", e))?;

        Ok(FxRate {
            from_currency: currency.to_string(),
            to_currency: "USD".to_string(),
            rate,
            source: FxRateSource::ExchangeRateApi,
            timestamp: chrono::Utc::now(),
        })
    }

    fn supported_currencies(&self) -> &[&str] {
        &["TWD"]
    }
}

/// Mock provider for testing with configurable static rates.
#[cfg(test)]
pub struct MockProvider {
    rates: std::collections::HashMap<String, Decimal>,
}

#[cfg(test)]
impl MockProvider {
    pub fn new(rates: Vec<(&str, Decimal)>) -> Self {
        Self {
            rates: rates.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl FxRateProvider for MockProvider {
    async fn get_usd_rate(&self, currency: &str) -> Result<FxRate, anyhow::Error> {
        let rate = self
            .rates
            .get(currency)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("MockProvider: no rate for {}", currency))?;
        Ok(FxRate {
            from_currency: currency.to_string(),
            to_currency: "USD".to_string(),
            rate,
            source: FxRateSource::Mock,
            timestamp: chrono::Utc::now(),
        })
    }

    fn supported_currencies(&self) -> &[&str] {
        // MockProvider supports whatever rates were configured
        // Return a static slice for the trait; callers should check `rates` directly
        &["BTC", "ETH", "USDT", "TWD"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_fx_rate_source_display() {
        assert_eq!(FxRateSource::Static.to_string(), "static");
        assert_eq!(FxRateSource::CoinGecko.to_string(), "coingecko");
        assert_eq!(
            FxRateSource::ExchangeRateApi.to_string(),
            "exchangerate-api"
        );
        assert_eq!(FxRateSource::Backfill.to_string(), "backfill");
        assert_eq!(FxRateSource::Mock.to_string(), "mock");
    }

    #[test]
    fn test_coingecko_coin_id_mapping() {
        assert_eq!(CoinGeckoProvider::coin_id("BTC"), Some("bitcoin"));
        assert_eq!(CoinGeckoProvider::coin_id("ETH"), Some("ethereum"));
        assert_eq!(CoinGeckoProvider::coin_id("USDT"), Some("tether"));
        assert_eq!(CoinGeckoProvider::coin_id("DOGE"), None);
    }

    #[tokio::test]
    async fn test_mock_provider_returns_configured_rates() {
        let provider = MockProvider::new(vec![("BTC", dec!(65000)), ("TWD", dec!(0.03125))]);
        let rate = provider.get_usd_rate("BTC").await.unwrap();
        assert_eq!(rate.rate, dec!(65000));
        assert_eq!(rate.source, FxRateSource::Mock);
        assert_eq!(rate.from_currency, "BTC");
    }

    #[tokio::test]
    async fn test_mock_provider_unknown_currency_fails() {
        let provider = MockProvider::new(vec![]);
        assert!(provider.get_usd_rate("XYZ").await.is_err());
    }

    #[tokio::test]
    async fn test_convert_to_usd_static() {
        // USD → USD is always 1:1
        let redis_client =
            Arc::new(redis::Client::open("redis://localhost:6379").expect("redis client"));
        let service = FxRateService::new(vec![], redis_client);
        let result = service.convert_to_usd(dec!(1000.50), "USD").await;
        let conversion = result.unwrap();
        assert_eq!(conversion.fx_rate_to_usd, Decimal::ONE);
        assert_eq!(conversion.amount_usd, dec!(1000.50));
        assert_eq!(conversion.source, FxRateSource::Static);
    }

    #[test]
    fn test_usd_conversion_rounding() {
        // Verify rounding: 0.5 BTC * 65432.10 = 32716.05
        let amount = dec!(0.5);
        let rate = dec!(65432.10);
        let usd = (amount * rate).round_dp(2);
        assert_eq!(usd, dec!(32716.05));
    }

    #[test]
    fn test_usd_conversion_tiny_fiat() {
        // TWD: 50000 * 0.03125 = 1562.50
        let amount = dec!(50000);
        let rate = dec!(0.03125);
        let usd = (amount * rate).round_dp(2);
        assert_eq!(usd, dec!(1562.50));
    }

    #[test]
    fn test_usd_conversion_stablecoin() {
        // USDT: 5000 * 1.0001 = 5000.50
        let amount = dec!(5000);
        let rate = dec!(1.0001);
        let usd = (amount * rate).round_dp(2);
        assert_eq!(usd, dec!(5000.50));
    }

    #[test]
    fn test_fx_rate_serialization() {
        let rate = FxRate {
            from_currency: "BTC".to_string(),
            to_currency: "USD".to_string(),
            rate: dec!(65432.10),
            source: FxRateSource::CoinGecko,
            timestamp: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&rate).unwrap();
        let deserialized: FxRate = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.rate, dec!(65432.10));
        assert_eq!(deserialized.source, FxRateSource::CoinGecko);
    }
}
