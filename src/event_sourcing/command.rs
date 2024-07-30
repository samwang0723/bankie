use uuid::Uuid;

use crate::common::money::Money;

#[derive(Debug)]
pub enum BankAccountCommand {
    OpenAccount { account_id: Uuid },
    ApproveAccount { account_id: Uuid, ledger_id: Uuid },
    Deposit { amount: Money },
    Withdrawl { amount: Money },
}

#[derive(Debug)]
pub enum BalanceCommand {
    Credit {
        ledger_id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
    Debit {
        ledger_id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
}
