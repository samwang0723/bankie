use async_trait::async_trait;

use crate::common::money::Money;

pub struct BankAccountServices {
    pub services: Box<dyn BankAccountApi>,
}

impl BankAccountServices {
    pub fn new(services: Box<dyn BankAccountApi>) -> Self {
        Self { services }
    }
}

// External services must be called during the processing of the command.
#[async_trait]
pub trait BankAccountApi: Sync + Send {
    async fn atm_withdrawal(&self, atm_id: &str, amount: Money) -> Result<(), AtmError>;
    async fn validate_check(&self, account_id: &str, check: &str) -> Result<(), CheckingError>;
}
pub struct AtmError;
pub struct CheckingError;

// A very simple "happy path" set of services that always succeed.
pub struct HappyPathBankAccountServices;

#[async_trait]
impl BankAccountApi for HappyPathBankAccountServices {
    async fn atm_withdrawal(&self, _atm_id: &str, _amount: Money) -> Result<(), AtmError> {
        Ok(())
    }

    async fn validate_check(
        &self,
        _account_id: &str,
        _check_number: &str,
    ) -> Result<(), CheckingError> {
        Ok(())
    }
}
