use serde::Deserialize;

use crate::common::money::Money;

#[derive(Debug, Deserialize)]
pub enum BankAccountCommand {
    OpenAccount { account_id: String },
    DepositMoney { amount: Money },
    WithdrawMoney { amount: Money },
    WriteCheck { check_number: String, amount: Money },
}
