use async_trait::async_trait;
use cqrs_es::persist::GenericQuery;
use cqrs_es::{EventEnvelope, Query, View};
use postgres_es::PostgresViewRepository;
use rust_decimal::Decimal;
use tracing::trace;

use crate::common::money::Money;
use crate::domain::events::{BankAccountEvent, LedgerEvent};
use crate::domain::models::{BankAccount, BankAccountStatus, BankAccountView, Ledger, LedgerView};
use crate::event_sourcing::event::Event;

pub struct AccountLogging {}

#[async_trait]
impl Query<BankAccount> for AccountLogging {
    async fn dispatch(&self, aggregate_id: &str, events: &[EventEnvelope<BankAccount>]) {
        for event in events {
            trace!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
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
                kind,
                user_id,
                currency,
            } => {
                self.id = base_event.get_aggregate_id();
                self.parent_id = base_event.get_parent_id();
                self.status = BankAccountStatus::Pending;
                self.created_at = base_event.get_created_at();
                self.updated_at = base_event.get_created_at();
                self.account_type = *account_type;
                self.kind = *kind;
                self.currency = *currency;
                self.user_id = user_id.clone();
            }
            BankAccountEvent::AccountKycApproved {
                ledger_id,
                base_event,
            } => {
                self.id = base_event.get_aggregate_id();
                self.parent_id = base_event.get_parent_id();
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
            trace!("{}-{}\n{:#?}", aggregate_id, event.sequence, &event.payload);
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
            LedgerEvent::LedgerInitiated { base_event, amount } => {
                self.id = base_event.get_aggregate_id();
                self.account_id = base_event.get_parent_id();
                self.created_at = base_event.get_created_at();
                self.updated_at = base_event.get_created_at();
                self.available = *amount;
                self.pending = Money::new(Decimal::ZERO, amount.currency);
                self.current = *amount;
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

#[cfg(test)]
mod tests {
    use crate::{common::money::Currency, event_sourcing::event::BaseEvent};

    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    #[test]
    fn test_update_with_ledger_initiated() {
        let mut ledger_view = LedgerView::default();
        let base_event = BaseEvent {
            aggregate_id: "ledger1".to_string(),
            parent_id: "account1".to_string(),
            created_at: Utc::now().to_string(),
        };
        let amount = Money::new(Decimal::new(1000, 2), Currency::USD);
        let event = EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 1,
            payload: LedgerEvent::LedgerInitiated {
                base_event: base_event.clone(),
                amount,
            },
        };

        ledger_view.update(&event);

        assert_eq!(ledger_view.id, base_event.get_aggregate_id());
        assert_eq!(ledger_view.account_id, base_event.get_parent_id());
        assert_eq!(ledger_view.created_at, base_event.get_created_at());
        assert_eq!(ledger_view.updated_at, base_event.get_created_at());
        assert_eq!(ledger_view.available, amount);
        assert_eq!(
            ledger_view.pending,
            Money::new(Decimal::ZERO, amount.currency)
        );
        assert_eq!(ledger_view.current, amount);
    }

    #[test]
    fn test_update_with_ledger_updated() {
        let mut ledger_view = LedgerView::default();
        let base_event = BaseEvent {
            aggregate_id: "ledger1".to_string(),
            parent_id: "account1".to_string(),
            created_at: Utc::now().to_string(),
        };
        let available_delta = Money::new(Decimal::new(500, 2), Currency::USD);
        let pending_delta = Money::new(Decimal::new(-200, 2), Currency::USD);
        let event = EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 2,
            payload: LedgerEvent::LedgerUpdated {
                amount: available_delta,
                transaction_id: "transaction1".to_string(),
                transaction_type: "credit".to_string(),
                available_delta,
                pending_delta,
                base_event: base_event.clone(),
            },
        };

        ledger_view.update(&event);

        assert_eq!(ledger_view.id, base_event.get_aggregate_id());
        assert_eq!(ledger_view.account_id, base_event.get_parent_id());
        assert_eq!(ledger_view.available, available_delta);
        assert_eq!(ledger_view.pending, pending_delta);
        assert_eq!(ledger_view.current, available_delta + pending_delta);
        assert_eq!(ledger_view.updated_at, base_event.get_created_at());
    }
}
