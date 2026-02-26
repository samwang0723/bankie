use anyhow::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct BankAccountError(String);

impl Display for BankAccountError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BankAccountError {}

impl From<&str> for BankAccountError {
    fn from(message: &str) -> Self {
        BankAccountError(message.to_string())
    }
}

impl From<Error> for BankAccountError {
    fn from(err: Error) -> Self {
        BankAccountError(err.to_string())
    }
}

#[derive(Debug)]
pub struct LedgerError(String);

impl Display for LedgerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for LedgerError {}

impl From<&str> for LedgerError {
    fn from(message: &str) -> Self {
        LedgerError(message.to_string())
    }
}
