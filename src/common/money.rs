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
