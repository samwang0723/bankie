use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::ops::{Add, Sub};
use std::str::FromStr;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    #[default]
    USD,
    TWD,
}

impl Currency {
    pub fn precision(&self) -> u32 {
        match self {
            Currency::USD => 2,
            Currency::TWD => 0,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Currency::USD => write!(f, "USD"),
            Currency::TWD => write!(f, "TWD"),
        }
    }
}

impl FromStr for Currency {
    type Err = CurrencyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "USD" => Ok(Currency::USD),
            "TWD" => Ok(Currency::TWD),
            _ => Err(CurrencyParseError),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrencyParseError;

impl fmt::Display for CurrencyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid currency")
    }
}

impl Error for CurrencyParseError {}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

/// Convert decimal into Money type with precision.
/// let usd_amount = Money::new(dec!(100.00), Currency::USD);
/// let twd_amount = Money::new(dec!(100), Currency::TWD);
impl Money {
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Money { amount, currency }
    }
}

impl Add for Money {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        if self.currency != other.currency {
            panic!("Cannot add Money with different currencies");
        }
        Money {
            amount: self.amount + other.amount,
            currency: self.currency,
        }
    }
}

impl Sub for Money {
    type Output = Self;

    fn sub(self, other: Self) -> Self::Output {
        if self.currency != other.currency {
            panic!("Cannot subtract Money with different currencies");
        }
        Money {
            amount: self.amount - other.amount,
            currency: self.currency,
        }
    }
}

impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self.currency != other.currency {
            return None;
        }
        self.amount.partial_cmp(&other.amount)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let precision = self.currency.precision();
        write!(
            f,
            "{:.precision$}",
            self.amount,
            precision = precision as usize
        )
    }
}

#[cfg(test)]
mod money_tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_currency_default() {
        let default_currency = Currency::default();
        assert_eq!(default_currency, Currency::USD);
    }

    #[test]
    fn test_money_default() {
        let default_money = Money::default();
        assert_eq!(default_money.amount, dec!(0));
        assert_eq!(default_money.currency, Currency::USD);
    }

    #[test]
    fn test_currency_from_str() {
        assert_eq!(Currency::from_str("USD").unwrap(), Currency::USD);
        assert_eq!(Currency::from_str("TWD").unwrap(), Currency::TWD);
        assert!(Currency::from_str("EUR").is_err());
    }

    #[test]
    fn test_currency_precision() {
        assert_eq!(Currency::USD.precision(), 2);
        assert_eq!(Currency::TWD.precision(), 0);
    }

    #[test]
    fn test_money_new() {
        let usd_amount = Money::new(dec!(100.00), Currency::USD);
        assert_eq!(usd_amount.amount, dec!(100.00));
        assert_eq!(usd_amount.currency, Currency::USD);

        let twd_amount = Money::new(dec!(100), Currency::TWD);
        assert_eq!(twd_amount.amount, dec!(100));
        assert_eq!(twd_amount.currency, Currency::TWD);
    }

    #[test]
    fn test_money_addition() {
        let usd1 = Money::new(dec!(50.00), Currency::USD);
        let usd2 = Money::new(dec!(25.00), Currency::USD);
        let result = usd1 + usd2;
        assert_eq!(result.amount, dec!(75.00));
        assert_eq!(result.currency, Currency::USD);
    }

    #[test]
    #[should_panic(expected = "Cannot add Money with different currencies")]
    fn test_money_addition_different_currencies() {
        let usd = Money::new(dec!(50.00), Currency::USD);
        let twd = Money::new(dec!(50), Currency::TWD);
        let _ = usd + twd;
    }

    #[test]
    fn test_money_subtraction() {
        let usd1 = Money::new(dec!(50.00), Currency::USD);
        let usd2 = Money::new(dec!(25.00), Currency::USD);
        let result = usd1 - usd2;
        assert_eq!(result.amount, dec!(25.00));
        assert_eq!(result.currency, Currency::USD);
    }

    #[test]
    #[should_panic(expected = "Cannot subtract Money with different currencies")]
    fn test_money_subtraction_different_currencies() {
        let usd = Money::new(dec!(50.00), Currency::USD);
        let twd = Money::new(dec!(50), Currency::TWD);
        let _ = usd - twd;
    }

    #[test]
    fn test_money_partial_cmp() {
        let usd1 = Money::new(dec!(50.00), Currency::USD);
        let usd2 = Money::new(dec!(25.00), Currency::USD);
        let usd3 = Money::new(dec!(50.00), Currency::USD);
        assert!(usd1 > usd2);
        assert!(usd2 < usd1);
        assert!(usd1 == usd3);
        assert!(usd1.partial_cmp(&usd2).is_some());
        assert!(usd1.partial_cmp(&usd2) == Some(Ordering::Greater));
        assert!(usd2.partial_cmp(&usd1) == Some(Ordering::Less));
        assert!(usd1.partial_cmp(&usd3) == Some(Ordering::Equal));
    }

    #[test]
    fn test_money_partial_cmp_different_currencies() {
        let usd = Money::new(dec!(50.00), Currency::USD);
        let twd = Money::new(dec!(50), Currency::TWD);
        assert!(usd.partial_cmp(&twd).is_none());
    }

    #[test]
    fn test_money_display() {
        let usd = Money::new(dec!(50.00), Currency::USD);
        let twd = Money::new(dec!(50), Currency::TWD);
        assert_eq!(format!("{}", usd), "50.00");
        assert_eq!(format!("{}", twd), "50");
    }

    #[test]
    fn test_money_display_zero_usd() {
        let money = Money::new(dec!(0), Currency::USD);
        assert_eq!(format!("{}", money), "0.00");
    }

    #[test]
    fn test_money_display_zero_twd() {
        let money = Money::new(dec!(0), Currency::TWD);
        assert_eq!(format!("{}", money), "0");
    }
}
