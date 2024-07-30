use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;
use rust_decimal::Decimal;

use crate::common::money::{Currency, Money};
use crate::domain::events::LedgerEvent;
use crate::domain::models::{Ledger, LedgerView, Transaction};
use crate::event_sourcing::event::Event;

pub struct LedgerLogging {}

#[async_trait]
impl Query<Ledger> for LedgerLogging {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Ledger>]) {
        for event in events {
            println!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
        }
    }
}

// Our second query, this one will be handled with Postgres `GenericQuery`
// which will serialize and persist our view after it is updated. It also
// provides a `load` method to deserialize the view on request.
pub type LedgerQuery = GenericQuery<PostgresViewRepository<LedgerView, Ledger>, LedgerView, Ledger>;

// This updates the view with events as they are committed.
impl View<Ledger> for LedgerView {
    fn update(&mut self, event: &EventEnvelope<Ledger>) {
        match &event.payload {
            LedgerEvent::LedgerDebited { amount, base_event } => {
                let account_id = base_event.get_parent_id();
                self.id = base_event.get_aggregate_id();
                self.account_id = Some(account_id.clone());
                self.available = self.available - *amount;
                self.current = self.available + self.pending;
                self.transactions.push(Transaction::new(
                    &account_id,
                    Money::new(Decimal::ZERO, Currency::USD),
                    *amount,
                    base_event.get_created_at(),
                ));
            }
            LedgerEvent::LedgerCredited { amount, base_event } => {
                let account_id = base_event.get_parent_id();
                self.id = base_event.get_aggregate_id();
                self.account_id = Some(account_id.clone());

                self.available = self.available + *amount;
                self.current = self.available + self.pending;
                self.transactions.push(Transaction::new(
                    &account_id,
                    *amount,
                    Money::new(Decimal::ZERO, Currency::USD),
                    base_event.get_created_at(),
                ));
            }
        }
    }
}
