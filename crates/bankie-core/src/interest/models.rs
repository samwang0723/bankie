use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use std::fmt;
use uuid::Uuid;

// =============================================================================
// Enums
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostingFrequency {
    Daily,
    Weekly,
    Monthly,
}

impl PostingFrequency {
    /// Returns true if a posting is due on the given date.
    /// - Daily: always due
    /// - Weekly: due when weekday matches posting_day (1=Mon..7=Sun)
    /// - Monthly: due when day-of-month matches posting_day
    pub fn is_posting_due(&self, date: NaiveDate, posting_day: Option<i16>) -> bool {
        match self {
            PostingFrequency::Daily => true,
            PostingFrequency::Weekly => {
                let Some(day) = posting_day else {
                    return false;
                };
                // chrono: Mon=1..Sun=7 via number_from_monday()
                date.weekday().number_from_monday() as i16 == day
            }
            PostingFrequency::Monthly => {
                let Some(day) = posting_day else {
                    return false;
                };
                date.day() as i16 == day
            }
        }
    }
}

impl fmt::Display for PostingFrequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PostingFrequency::Daily => write!(f, "Daily"),
            PostingFrequency::Weekly => write!(f, "Weekly"),
            PostingFrequency::Monthly => write!(f, "Monthly"),
        }
    }
}

impl std::str::FromStr for PostingFrequency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Daily" => Ok(PostingFrequency::Daily),
            "Weekly" => Ok(PostingFrequency::Weekly),
            "Monthly" => Ok(PostingFrequency::Monthly),
            _ => Err(format!("invalid posting frequency: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DayCountConvention {
    #[serde(rename = "Actual/365")]
    Actual365,
    #[serde(rename = "Actual/360")]
    Actual360,
    #[serde(rename = "30/360")]
    Thirty360,
}

impl DayCountConvention {
    pub fn days_in_year(&self) -> u32 {
        match self {
            DayCountConvention::Actual365 => 365,
            DayCountConvention::Actual360 | DayCountConvention::Thirty360 => 360,
        }
    }
}

impl fmt::Display for DayCountConvention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DayCountConvention::Actual365 => write!(f, "Actual/365"),
            DayCountConvention::Actual360 => write!(f, "Actual/360"),
            DayCountConvention::Thirty360 => write!(f, "30/360"),
        }
    }
}

impl std::str::FromStr for DayCountConvention {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Actual/365" => Ok(DayCountConvention::Actual365),
            "Actual/360" => Ok(DayCountConvention::Actual360),
            "30/360" => Ok(DayCountConvention::Thirty360),
            _ => Err(format!("invalid day count convention: {}", s)),
        }
    }
}

// =============================================================================
// DB row types
// =============================================================================

#[derive(FromRow, Debug, Clone, Serialize)]
pub struct InterestRateConfig {
    pub id: Uuid,
    pub currency: String,
    pub account_kind: String,
    pub day_count: String,
    pub posting_frequency: String,
    pub posting_day: Option<i16>,
    pub effective_from: NaiveDate,
    pub effective_to: Option<NaiveDate>,
    pub is_active: bool,
}

impl InterestRateConfig {
    pub fn day_count_convention(&self) -> Result<DayCountConvention, String> {
        self.day_count.parse()
    }

    pub fn posting_freq(&self) -> Result<PostingFrequency, String> {
        self.posting_frequency.parse()
    }
}

#[derive(FromRow, Debug, Clone, Serialize)]
pub struct InterestRateTier {
    pub id: Uuid,
    pub rate_config_id: Uuid,
    pub tier_order: i16,
    pub min_balance: Decimal,
    pub max_balance: Option<Decimal>,
    pub apr: Decimal,
}

#[derive(FromRow, Debug, Clone, Serialize)]
pub struct InterestAccrual {
    pub id: Uuid,
    pub tenant_id: i32,
    pub account_id: String,
    pub ledger_id: String,
    pub currency: String,
    pub accrual_date: NaiveDate,
    pub balance_used: Decimal,
    pub daily_interest: Decimal,
    pub rate_config_id: Uuid,
    pub tier_breakdown: serde_json::Value,
}

#[derive(FromRow, Debug, Clone, Serialize)]
pub struct InterestPosting {
    pub id: Uuid,
    pub tenant_id: i32,
    pub account_id: String,
    pub currency: String,
    pub posting_date: NaiveDate,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub accrued_total: Decimal,
    pub posted_amount: Decimal,
    pub transaction_id: Option<Uuid>,
    pub status: String,
    pub error_message: Option<String>,
}

// =============================================================================
// Tier breakdown for JSONB serialization
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TierBreakdown {
    pub tier_order: i16,
    pub min_balance: Decimal,
    pub max_balance: Option<Decimal>,
    pub apr: Decimal,
    pub applicable_amount: Decimal,
    pub daily_interest: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn test_posting_frequency_daily_always_due() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        assert!(PostingFrequency::Daily.is_posting_due(date, None));
    }

    #[test]
    fn test_posting_frequency_weekly_correct_day() {
        // 2026-03-02 is Monday (number_from_monday = 1)
        let monday = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        assert!(PostingFrequency::Weekly.is_posting_due(monday, Some(1)));
        assert!(!PostingFrequency::Weekly.is_posting_due(monday, Some(5)));
    }

    #[test]
    fn test_posting_frequency_weekly_no_posting_day() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
        assert!(!PostingFrequency::Weekly.is_posting_due(date, None));
    }

    #[test]
    fn test_posting_frequency_monthly_correct_day() {
        let first = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert!(PostingFrequency::Monthly.is_posting_due(first, Some(1)));
        assert!(!PostingFrequency::Monthly.is_posting_due(first, Some(15)));
    }

    #[test]
    fn test_posting_frequency_monthly_no_posting_day() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        assert!(!PostingFrequency::Monthly.is_posting_due(date, None));
    }

    #[test]
    fn test_day_count_convention_days_in_year() {
        assert_eq!(DayCountConvention::Actual365.days_in_year(), 365);
        assert_eq!(DayCountConvention::Actual360.days_in_year(), 360);
        assert_eq!(DayCountConvention::Thirty360.days_in_year(), 360);
    }

    #[test]
    fn test_day_count_convention_parse() {
        assert_eq!(
            "Actual/365".parse::<DayCountConvention>().unwrap(),
            DayCountConvention::Actual365
        );
        assert_eq!(
            "Actual/360".parse::<DayCountConvention>().unwrap(),
            DayCountConvention::Actual360
        );
        assert_eq!(
            "30/360".parse::<DayCountConvention>().unwrap(),
            DayCountConvention::Thirty360
        );
        assert!("invalid".parse::<DayCountConvention>().is_err());
    }

    #[test]
    fn test_posting_frequency_parse() {
        assert_eq!(
            "Daily".parse::<PostingFrequency>().unwrap(),
            PostingFrequency::Daily
        );
        assert_eq!(
            "Weekly".parse::<PostingFrequency>().unwrap(),
            PostingFrequency::Weekly
        );
        assert_eq!(
            "Monthly".parse::<PostingFrequency>().unwrap(),
            PostingFrequency::Monthly
        );
        assert!("invalid".parse::<PostingFrequency>().is_err());
    }

    #[test]
    fn test_day_count_convention_display() {
        assert_eq!(DayCountConvention::Actual365.to_string(), "Actual/365");
        assert_eq!(DayCountConvention::Actual360.to_string(), "Actual/360");
        assert_eq!(DayCountConvention::Thirty360.to_string(), "30/360");
    }

    #[test]
    fn test_posting_frequency_display() {
        assert_eq!(PostingFrequency::Daily.to_string(), "Daily");
        assert_eq!(PostingFrequency::Weekly.to_string(), "Weekly");
        assert_eq!(PostingFrequency::Monthly.to_string(), "Monthly");
    }

    #[test]
    fn test_rate_config_helpers() {
        let config = InterestRateConfig {
            id: Uuid::new_v4(),
            currency: "USD".to_string(),
            account_kind: "Interest".to_string(),
            day_count: "Actual/365".to_string(),
            posting_frequency: "Monthly".to_string(),
            posting_day: Some(1),
            effective_from: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            effective_to: None,
            is_active: true,
        };
        assert_eq!(
            config.day_count_convention().unwrap(),
            DayCountConvention::Actual365
        );
        assert_eq!(config.posting_freq().unwrap(), PostingFrequency::Monthly);
    }
}
