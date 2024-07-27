use serde::{Deserialize, Serialize};

use crate::common::money::Money;

#[derive(Serialize, Default, Deserialize)]
pub struct BankAccount {
    pub opened: bool,
    pub balance: Money,
}
