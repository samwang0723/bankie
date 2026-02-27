use rust_decimal::Decimal;
use serde::Serialize;

#[derive(Serialize, Default, sqlx::FromRow)]
pub struct BankAccountWithLedger {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_balance: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<i32>,
}
