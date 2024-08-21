use rust_decimal::Decimal;
use serde::Serialize;

#[derive(Serialize, Default, sqlx::FromRow)]
pub struct BankAccountWithLedger {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}
