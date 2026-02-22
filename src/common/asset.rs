use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetClass {
    Fiat,
    Crypto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub code: String,
    pub asset_class: AssetClass,
    pub precision: u32,
    pub min_amount: Decimal,
    pub display_name: String,
    pub network: Option<String>,
    pub is_active: bool,
}

/// Thread-safe registry of supported assets, loaded from DB at startup.
#[derive(Clone)]
pub struct AssetRegistry {
    inner: Arc<RwLock<HashMap<String, Asset>>>,
}

impl AssetRegistry {
    pub fn new(assets: Vec<Asset>) -> Self {
        let map: HashMap<String, Asset> = assets.into_iter().map(|a| (a.code.clone(), a)).collect();
        Self {
            inner: Arc::new(RwLock::new(map)),
        }
    }

    /// Create an empty registry (for tests).
    pub fn empty() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a registry with default fiat assets (for backward compatibility / tests).
    pub fn with_defaults() -> Self {
        let assets = vec![
            Asset {
                code: "USD".to_string(),
                asset_class: AssetClass::Fiat,
                precision: 2,
                min_amount: Decimal::new(1, 2),
                display_name: "US Dollar".to_string(),
                network: None,
                is_active: true,
            },
            Asset {
                code: "TWD".to_string(),
                asset_class: AssetClass::Fiat,
                precision: 0,
                min_amount: Decimal::ONE,
                display_name: "Taiwan Dollar".to_string(),
                network: None,
                is_active: true,
            },
        ];
        Self::new(assets)
    }

    pub async fn get(&self, code: &str) -> Option<Asset> {
        let map = self.inner.read().await;
        map.get(code).cloned()
    }

    pub async fn precision(&self, code: &str) -> Option<u32> {
        let map = self.inner.read().await;
        map.get(code).map(|a| a.precision)
    }

    pub async fn validate_asset_code(&self, code: &str) -> bool {
        let map = self.inner.read().await;
        map.get(code).is_some_and(|a| a.is_active)
    }

    pub async fn is_crypto(&self, code: &str) -> bool {
        let map = self.inner.read().await;
        map.get(code)
            .is_some_and(|a| a.asset_class == AssetClass::Crypto)
    }

    /// Reload assets from a new list (for hot-reload from DB).
    pub async fn reload(&self, assets: Vec<Asset>) {
        let mut map = self.inner.write().await;
        map.clear();
        for asset in assets {
            map.insert(asset.code.clone(), asset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_defaults() {
        let registry = AssetRegistry::with_defaults();
        assert!(registry.validate_asset_code("USD").await);
        assert!(registry.validate_asset_code("TWD").await);
        assert!(!registry.validate_asset_code("BTC").await);
    }

    #[tokio::test]
    async fn test_registry_precision() {
        let registry = AssetRegistry::with_defaults();
        assert_eq!(registry.precision("USD").await, Some(2));
        assert_eq!(registry.precision("TWD").await, Some(0));
        assert_eq!(registry.precision("BTC").await, None);
    }

    #[tokio::test]
    async fn test_registry_with_crypto() {
        let assets = vec![
            Asset {
                code: "BTC".to_string(),
                asset_class: AssetClass::Crypto,
                precision: 8,
                min_amount: Decimal::new(1, 8),
                display_name: "Bitcoin".to_string(),
                network: None,
                is_active: true,
            },
            Asset {
                code: "USD".to_string(),
                asset_class: AssetClass::Fiat,
                precision: 2,
                min_amount: Decimal::new(1, 2),
                display_name: "US Dollar".to_string(),
                network: None,
                is_active: true,
            },
        ];
        let registry = AssetRegistry::new(assets);
        assert!(registry.is_crypto("BTC").await);
        assert!(!registry.is_crypto("USD").await);
        assert_eq!(registry.precision("BTC").await, Some(8));
    }

    #[tokio::test]
    async fn test_registry_reload() {
        let registry = AssetRegistry::with_defaults();
        assert!(registry.validate_asset_code("USD").await);

        let new_assets = vec![Asset {
            code: "ETH".to_string(),
            asset_class: AssetClass::Crypto,
            precision: 18,
            min_amount: Decimal::new(1, 18),
            display_name: "Ethereum".to_string(),
            network: Some("ethereum".to_string()),
            is_active: true,
        }];
        registry.reload(new_assets).await;

        assert!(!registry.validate_asset_code("USD").await);
        assert!(registry.validate_asset_code("ETH").await);
    }
}
