use uuid::Uuid;

use crate::{common::money::Money, domain::models::BankAccountType};

#[derive(Debug)]
pub enum BankAccountCommand {
    OpenAccount {
        id: Uuid,
        account_type: BankAccountType,
    },
    ApproveAccount {
        id: Uuid,
        ledger_id: Uuid,
    },
    Deposit {
        amount: Money,
    },
    Withdrawl {
        amount: Money,
    },
}

#[derive(Debug)]
pub enum LedgerCommand {
    Init {
        id: Uuid,
        account_id: Uuid,
    },
    Credit {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
    Debit {
        id: Uuid,
        account_id: Uuid,
        transaction_id: Uuid,
        amount: Money,
    },
}
