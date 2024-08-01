use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;

use crate::domain::events::{BankAccountEvent, LedgerEvent};
use crate::domain::models::{BankAccount, BankAccountStatus, BankAccountView, Ledger, LedgerView};
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
            BankAccountEvent::AccountOpened {
                base_event,
                account_type,
            } => {
                self.id = base_event.get_aggregate_id();
                self.status = BankAccountStatus::Pending;
                self.created_at = base_event.get_created_at();
                self.updated_at = base_event.get_created_at();
                self.account_type = *account_type;
            }
            BankAccountEvent::AccountKycApproved {
                ledger_id,
                base_event,
            } => {
                self.id = base_event.get_aggregate_id();
                self.ledger_id = ledger_id.clone();
                self.status = BankAccountStatus::Approved;
                self.updated_at = base_event.get_created_at();
            }
            BankAccountEvent::CustomerDepositedCash { .. } => {}
            BankAccountEvent::CustomerWithdrewCash { .. } => {}
        }
    }
}

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
            LedgerEvent::LedgerInitiated { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.account_id = base_event.get_parent_id();
                self.created_at = base_event.get_created_at();
                self.updated_at = base_event.get_created_at();
            }
            LedgerEvent::LedgerUpdated {
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
                self.current = self.available + self.pending;
                self.updated_at = base_event.get_created_at();
            }
        }
    }
}
