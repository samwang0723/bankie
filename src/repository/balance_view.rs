use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;

use crate::domain::events::BalanceEvent;
use crate::domain::models::{Balance, BalanceView};
use crate::event_sourcing::event::Event;

pub struct BalanceLogging {}

#[async_trait]
impl Query<Balance> for BalanceLogging {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<Balance>]) {
        for event in events {
            println!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
        }
    }
}

// Our second query, this one will be handled with Postgres `GenericQuery`
// which will serialize and persist our view after it is updated. It also
// provides a `load` method to deserialize the view on request.
pub type BalanceQuery =
    GenericQuery<PostgresViewRepository<BalanceView, Balance>, BalanceView, Balance>;

// This updates the view with events as they are committed.
impl View<Balance> for BalanceView {
    fn update(&mut self, event: &EventEnvelope<Balance>) {
        match &event.payload {
            BalanceEvent::BalanceInitiated { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.account_id = base_event.get_parent_id();
                self.created_at = base_event.get_created_at();
                self.updated_at = base_event.get_created_at();
            }
            BalanceEvent::BalanceChanged {
                amount: _,
                transaction_id: _,
                transaction_type: _,
                available_delta,
                pending_delta,
                base_event,
            } => {
                let account_id = base_event.get_parent_id();
                self.id = base_event.get_aggregate_id();
                self.account_id = account_id.clone();
                self.available = self.available + *available_delta;
                self.pending = self.pending + *pending_delta;
                self.current_balance = self.available + self.pending;
                self.updated_at = base_event.get_created_at();
            }
        }
    }
}
