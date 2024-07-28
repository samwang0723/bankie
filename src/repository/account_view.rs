use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;
use rust_decimal::Decimal;

use crate::common::money::{Currency, Money};
use crate::domain::models::{BankAccount, BankAccountView, LedgerEntry};
use crate::event_sourcing::event::BankAccountEvent;

pub struct LoggingQuery {}

#[async_trait]
impl Query<BankAccount> for LoggingQuery {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<BankAccount>]) {
        for event in events {
            println!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
        }
    }
}

// Our second query, this one will be handled with Postgres `GenericQuery`
// which will serialize and persist our view after it is updated. It also
// provides a `load` method to deserialize the view on request.
pub type AccountQuery = GenericQuery<
    PostgresViewRepository<BankAccountView, BankAccount>,
    BankAccountView,
    BankAccount,
>;

// This updates the view with events as they are committed.
// The logic should be minimal here, e.g., don't calculate the account balance,
// design the events to carry the balance information instead.
impl View<BankAccount> for BankAccountView {
    fn update(&mut self, event: &EventEnvelope<BankAccount>) {
        match &event.payload {
            BankAccountEvent::AccountOpened { account_id } => {
                self.account_id = Some(account_id.clone());
                self.balance = Money::new(Decimal::ZERO, Currency::USD);
            }
            BankAccountEvent::CustomerDepositedMoney { amount, balance } => {
                self.ledger.push(LedgerEntry::new("deposit", *amount));
                self.balance = *balance;
            }
            BankAccountEvent::CustomerWithdrewCash { amount, balance } => {
                self.ledger
                    .push(LedgerEntry::new("atm withdrawal", *amount));
                self.balance = *balance;
            }
            BankAccountEvent::CustomerWroteCheck {
                check_number,
                amount,
                balance,
            } => {
                self.ledger.push(LedgerEntry::new(check_number, *amount));
                self.written_checks.push(check_number.clone());
                self.balance = *balance;
            }
        }
    }
}
