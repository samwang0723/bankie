use rust_decimal::prelude::*;
use rust_decimal::Decimal;

use super::models::{DayCountConvention, InterestRateTier, TierBreakdown};
use crate::common::money::default_precision;

/// Calculate daily interest using tiered blended rate.
///
/// Each tier applies only to the portion of balance within that tier's range.
/// Returns (total daily interest at max precision, per-tier breakdown).
pub fn calculate_daily_interest(
    balance: Decimal,
    tiers: &[InterestRateTier],
    day_count: &DayCountConvention,
) -> (Decimal, Vec<TierBreakdown>) {
    if balance <= Decimal::ZERO || tiers.is_empty() {
        return (Decimal::ZERO, vec![]);
    }

    let days = Decimal::from(day_count.days_in_year());
    let mut remaining = balance;
    let mut total = Decimal::ZERO;
    let mut breakdowns = Vec::with_capacity(tiers.len());

    // Tiers must be sorted by tier_order (ascending)
    let mut sorted_tiers: Vec<&InterestRateTier> = tiers.iter().collect();
    sorted_tiers.sort_by_key(|t| t.tier_order);

    for tier in sorted_tiers {
        if remaining <= Decimal::ZERO {
            break;
        }

        let tier_range = match tier.max_balance {
            Some(max) => max - tier.min_balance,
            None => remaining, // unlimited top tier
        };

        let applicable = remaining.min(tier_range);
        let daily_interest = applicable * tier.apr / days;

        total += daily_interest;
        breakdowns.push(TierBreakdown {
            tier_order: tier.tier_order,
            min_balance: tier.min_balance,
            max_balance: tier.max_balance,
            apr: tier.apr,
            applicable_amount: applicable,
            daily_interest,
        });

        remaining -= applicable;
    }

    (total, breakdowns)
}

/// Round accrued total to currency precision using banker's rounding.
/// Only called at posting time — accrual stores max precision.
#[allow(dead_code)]
pub fn round_for_posting(accrued_total: Decimal, currency: &str) -> Decimal {
    let precision = default_precision(currency);
    accrued_total.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven)
}

/// Convert APR to APY for display (Reg DD compliance).
/// `compounding_periods` = 12 for monthly, 365 for daily, etc.
#[allow(dead_code)]
pub fn apr_to_apy(apr: Decimal, compounding_periods: u32) -> Decimal {
    if compounding_periods == 0 || apr == Decimal::ZERO {
        return apr;
    }
    let n = Decimal::from(compounding_periods);
    let base = Decimal::ONE + apr / n;
    // (1 + apr/n)^n - 1 using repeated multiplication
    let mut result = base;
    for _ in 1..compounding_periods {
        result *= base;
    }
    result - Decimal::ONE
}

/// Estimate total interest for a given balance over N days.
pub fn estimate_interest(
    balance: Decimal,
    tiers: &[InterestRateTier],
    day_count: &DayCountConvention,
    days: u32,
) -> Decimal {
    let (daily, _) = calculate_daily_interest(balance, tiers, day_count);
    daily * Decimal::from(days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_tier(order: i16, min: Decimal, max: Option<Decimal>, apr: Decimal) -> InterestRateTier {
        InterestRateTier {
            id: Uuid::new_v4(),
            rate_config_id: Uuid::new_v4(),
            tier_order: order,
            min_balance: min,
            max_balance: max,
            apr,
        }
    }

    #[test]
    fn test_single_tier_daily_interest() {
        // $10,000 at 4.5% APR, Actual/365
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0.045))];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(10000), &tiers, &DayCountConvention::Actual365);

        // 10000 * 0.045 / 365 = 1.232876712328...
        let expected = dec!(10000) * dec!(0.045) / dec!(365);
        assert_eq!(daily, expected);
        assert_eq!(breakdowns.len(), 1);
        assert_eq!(breakdowns[0].applicable_amount, dec!(10000));
    }

    #[test]
    fn test_blended_two_tiers() {
        // $50,000 with [0-10K: 4.5%, 10K-100K: 4.0%]
        let tiers = vec![
            make_tier(1, dec!(0), Some(dec!(10000)), dec!(0.045)),
            make_tier(2, dec!(10000), Some(dec!(100000)), dec!(0.04)),
        ];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(50000), &tiers, &DayCountConvention::Actual365);

        let tier1 = dec!(10000) * dec!(0.045) / dec!(365);
        let tier2 = dec!(40000) * dec!(0.04) / dec!(365);
        assert_eq!(daily, tier1 + tier2);
        assert_eq!(breakdowns.len(), 2);
        assert_eq!(breakdowns[0].applicable_amount, dec!(10000));
        assert_eq!(breakdowns[1].applicable_amount, dec!(40000));
    }

    #[test]
    fn test_blended_balance_exactly_at_boundary() {
        // $10,000 with [0-10K: 4.5%, 10K-100K: 4.0%] — only tier 1 applies
        let tiers = vec![
            make_tier(1, dec!(0), Some(dec!(10000)), dec!(0.045)),
            make_tier(2, dec!(10000), Some(dec!(100000)), dec!(0.04)),
        ];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(10000), &tiers, &DayCountConvention::Actual365);

        let expected = dec!(10000) * dec!(0.045) / dec!(365);
        assert_eq!(daily, expected);
        assert_eq!(breakdowns.len(), 1);
    }

    #[test]
    fn test_blended_three_tiers() {
        let tiers = vec![
            make_tier(1, dec!(0), Some(dec!(10000)), dec!(0.05)),
            make_tier(2, dec!(10000), Some(dec!(50000)), dec!(0.04)),
            make_tier(3, dec!(50000), None, dec!(0.03)),
        ];
        // $75,000 — spans all three tiers
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(75000), &tiers, &DayCountConvention::Actual365);

        let t1 = dec!(10000) * dec!(0.05) / dec!(365);
        let t2 = dec!(40000) * dec!(0.04) / dec!(365);
        let t3 = dec!(25000) * dec!(0.03) / dec!(365);
        assert_eq!(daily, t1 + t2 + t3);
        assert_eq!(breakdowns.len(), 3);
        assert_eq!(breakdowns[0].applicable_amount, dec!(10000));
        assert_eq!(breakdowns[1].applicable_amount, dec!(40000));
        assert_eq!(breakdowns[2].applicable_amount, dec!(25000));
    }

    #[test]
    fn test_zero_balance() {
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0.045))];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(0), &tiers, &DayCountConvention::Actual365);
        assert_eq!(daily, Decimal::ZERO);
        assert!(breakdowns.is_empty());
    }

    #[test]
    fn test_negative_balance() {
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0.045))];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(-100), &tiers, &DayCountConvention::Actual365);
        assert_eq!(daily, Decimal::ZERO);
        assert!(breakdowns.is_empty());
    }

    #[test]
    fn test_empty_tiers() {
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(10000), &[], &DayCountConvention::Actual365);
        assert_eq!(daily, Decimal::ZERO);
        assert!(breakdowns.is_empty());
    }

    #[test]
    fn test_zero_apr_tier() {
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0))];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(10000), &tiers, &DayCountConvention::Actual365);
        assert_eq!(daily, Decimal::ZERO);
        assert_eq!(breakdowns.len(), 1);
        assert_eq!(breakdowns[0].daily_interest, Decimal::ZERO);
    }

    #[test]
    fn test_actual_360_day_count() {
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0.036))];
        let (daily, _) =
            calculate_daily_interest(dec!(10000), &tiers, &DayCountConvention::Actual360);
        // 10000 * 0.036 / 360 = 1.0
        assert_eq!(daily, Decimal::ONE);
    }

    #[test]
    fn test_round_for_posting_usd() {
        // Banker's rounding: 0.5 → rounds to even
        assert_eq!(round_for_posting(dec!(12.345), "USD"), dec!(12.34)); // .5 rounds to even (4)
        assert_eq!(round_for_posting(dec!(12.355), "USD"), dec!(12.36)); // .5 rounds to even (6)
        assert_eq!(round_for_posting(dec!(12.346), "USD"), dec!(12.35));
    }

    #[test]
    fn test_round_for_posting_twd() {
        // TWD has 0 decimal precision
        assert_eq!(round_for_posting(dec!(123.4), "TWD"), dec!(123));
        assert_eq!(round_for_posting(dec!(123.5), "TWD"), dec!(124)); // .5 rounds to even (124)
        assert_eq!(round_for_posting(dec!(124.5), "TWD"), dec!(124)); // .5 rounds to even (124)
    }

    #[test]
    fn test_apr_to_apy_monthly() {
        // APR 4.5%, monthly compounding → APY ≈ 4.594%
        let apy = apr_to_apy(dec!(0.045), 12);
        // (1 + 0.045/12)^12 - 1 ≈ 0.04594
        assert!(apy > dec!(0.0459));
        assert!(apy < dec!(0.0460));
    }

    #[test]
    fn test_apr_to_apy_zero() {
        assert_eq!(apr_to_apy(dec!(0), 12), Decimal::ZERO);
    }

    #[test]
    fn test_apr_to_apy_zero_periods() {
        assert_eq!(apr_to_apy(dec!(0.045), 0), dec!(0.045));
    }

    #[test]
    fn test_estimate_interest_30_days() {
        let tiers = vec![make_tier(1, dec!(0), None, dec!(0.045))];
        let estimate = estimate_interest(dec!(10000), &tiers, &DayCountConvention::Actual365, 30);
        let expected = dec!(10000) * dec!(0.045) / dec!(365) * dec!(30);
        assert_eq!(estimate, expected);
    }

    #[test]
    fn test_unsorted_tiers_handled() {
        // Tiers given out of order — should still work (sorted by tier_order)
        let tiers = vec![
            make_tier(2, dec!(10000), None, dec!(0.04)),
            make_tier(1, dec!(0), Some(dec!(10000)), dec!(0.045)),
        ];
        let (daily, breakdowns) =
            calculate_daily_interest(dec!(20000), &tiers, &DayCountConvention::Actual365);

        let t1 = dec!(10000) * dec!(0.045) / dec!(365);
        let t2 = dec!(10000) * dec!(0.04) / dec!(365);
        assert_eq!(daily, t1 + t2);
        assert_eq!(breakdowns[0].tier_order, 1);
        assert_eq!(breakdowns[1].tier_order, 2);
    }
}
