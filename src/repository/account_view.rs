use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;

use crate::domain::events::BankAccountEvent;
use crate::domain::models::{BankAccount, BankAccountStatus, BankAccountView};
use crate::event_sourcing::event::Event;

pub struct AccountLogging {}

#[async_trait]
impl Query<BankAccount> for AccountLogging {
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
            BankAccountEvent::AccountOpened { base_event } => {
                self.account_id = base_event.get_aggregate_id();
                self.status = BankAccountStatus::Pending;
                self.updated_at = base_event.get_created_at();
            }
            BankAccountEvent::AccountKycApproved {
                ledger_id,
                base_event,
            } => {
                self.account_id = base_event.get_aggregate_id();
                self.ledger_id = ledger_id.clone();
                self.status = BankAccountStatus::Approved;
                self.updated_at = base_event.get_created_at();
            }
            BankAccountEvent::CustomerDepositedMoney { .. } => {}
            BankAccountEvent::CustomerWithdrewCash { .. } => {}
        }
    }
}
