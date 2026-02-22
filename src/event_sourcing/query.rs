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
                external_reference_id,
                account_number,
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
                self.external_reference_id = external_reference_id.clone();
                self.account_number = account_number.clone();
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
            BankAccountEvent::AccountFrozen { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = BankAccountStatus::Freeze;
                self.updated_at = base_event.get_created_at();
            }
            BankAccountEvent::AccountUnfrozen { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = BankAccountStatus::Approved;
                self.updated_at = base_event.get_created_at();
            }
            BankAccountEvent::AccountClosed { base_event } => {
                self.id = base_event.get_aggregate_id();
                self.status = BankAccountStatus::CustomerClosed;
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
    use crate::{
        common::money::Currency,
        domain::models::{BankAccountKind, BankAccountType},
        event_sourcing::event::BaseEvent,
    };

    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    // Q1: BankAccountView update for AccountOpened
    #[test]
    fn test_update_with_account_opened() {
        let mut view = BankAccountView::default();
        let base_event = BaseEvent {
            aggregate_id: "acc1".to_string(),
            parent_id: "".to_string(),
            created_at: Utc::now().to_string(),
        };
        let event = EventEnvelope {
            aggregate_id: "acc1".to_string(),
            metadata: Default::default(),
            sequence: 1,
            payload: BankAccountEvent::AccountOpened {
                base_event: base_event.clone(),
                account_type: BankAccountType::Retail,
                kind: BankAccountKind::Checking,
                external_reference_id: Some("user-123".to_string()),
                account_number: "123456789012".to_string(),
                currency: Currency::USD,
            },
        };
        view.update(&event);

        assert_eq!(view.id, "acc1");
        assert_eq!(view.status, BankAccountStatus::Pending);
        assert_eq!(view.account_type, BankAccountType::Retail);
        assert_eq!(view.kind, BankAccountKind::Checking);
        assert_eq!(view.currency, Currency::USD);
        assert_eq!(view.external_reference_id, Some("user-123".to_string()));
        assert_eq!(view.account_number, "123456789012");
    }

    // Q2: BankAccountView update for AccountKycApproved
    #[test]
    fn test_update_with_account_kyc_approved() {
        let mut view = BankAccountView::default();
        let base_event = BaseEvent {
            aggregate_id: "acc1".to_string(),
            parent_id: "parent1".to_string(),
            created_at: Utc::now().to_string(),
        };
        let event = EventEnvelope {
            aggregate_id: "acc1".to_string(),
            metadata: Default::default(),
            sequence: 2,
            payload: BankAccountEvent::AccountKycApproved {
                ledger_id: "ledger-123".to_string(),
                base_event: base_event.clone(),
            },
        };
        view.update(&event);

        assert_eq!(view.id, "acc1");
        assert_eq!(view.status, BankAccountStatus::Approved);
        assert_eq!(view.ledger_id, "ledger-123");
        assert_eq!(view.parent_id, "parent1");
    }

    // Q3: BankAccountView update for AccountFrozen, AccountUnfrozen, AccountClosed
    #[test]
    fn test_update_with_account_frozen() {
        let mut view = BankAccountView {
            status: BankAccountStatus::Approved,
            ..Default::default()
        };
        let base_event = BaseEvent {
            aggregate_id: "acc1".to_string(),
            parent_id: "".to_string(),
            created_at: Utc::now().to_string(),
        };
        let event = EventEnvelope {
            aggregate_id: "acc1".to_string(),
            metadata: Default::default(),
            sequence: 3,
            payload: BankAccountEvent::AccountFrozen {
                base_event: base_event.clone(),
            },
        };
        view.update(&event);
        assert_eq!(view.status, BankAccountStatus::Freeze);
    }

    #[test]
    fn test_update_with_account_unfrozen() {
        let mut view = BankAccountView {
            status: BankAccountStatus::Freeze,
            ..Default::default()
        };
        let base_event = BaseEvent {
            aggregate_id: "acc1".to_string(),
            parent_id: "".to_string(),
            created_at: Utc::now().to_string(),
        };
        let event = EventEnvelope {
            aggregate_id: "acc1".to_string(),
            metadata: Default::default(),
            sequence: 4,
            payload: BankAccountEvent::AccountUnfrozen {
                base_event: base_event.clone(),
            },
        };
        view.update(&event);
        assert_eq!(view.status, BankAccountStatus::Approved);
    }

    #[test]
    fn test_update_with_account_closed() {
        let mut view = BankAccountView {
            status: BankAccountStatus::Approved,
            ..Default::default()
        };
        let base_event = BaseEvent {
            aggregate_id: "acc1".to_string(),
            parent_id: "".to_string(),
            created_at: Utc::now().to_string(),
        };
        let event = EventEnvelope {
            aggregate_id: "acc1".to_string(),
            metadata: Default::default(),
            sequence: 5,
            payload: BankAccountEvent::AccountClosed {
                base_event: base_event.clone(),
            },
        };
        view.update(&event);
        assert_eq!(view.status, BankAccountStatus::CustomerClosed);
    }

    // Q4: LedgerView multi-event sequence (init + credit + debit_hold)
    #[test]
    fn test_ledger_view_multi_event_sequence() {
        let mut view = LedgerView::default();
        let base_event = BaseEvent {
            aggregate_id: "ledger1".to_string(),
            parent_id: "account1".to_string(),
            created_at: Utc::now().to_string(),
        };

        // Init with $1000
        view.update(&EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 1,
            payload: LedgerEvent::LedgerInitiated {
                base_event: base_event.clone(),
                amount: Money::new(Decimal::new(1000, 0), Currency::USD),
            },
        });
        assert_eq!(view.available.amount, Decimal::new(1000, 0));
        assert_eq!(view.pending.amount, Decimal::ZERO);
        assert_eq!(view.current.amount, Decimal::new(1000, 0));

        // Credit $500 (hold + release)
        view.update(&EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 2,
            payload: LedgerEvent::LedgerUpdated {
                amount: Money::new(Decimal::new(500, 0), Currency::USD),
                transaction_id: "tx1".to_string(),
                transaction_type: "credit_hold".to_string(),
                available_delta: Money::new(Decimal::ZERO, Currency::USD),
                pending_delta: Money::new(Decimal::new(500, 0), Currency::USD),
                base_event: base_event.clone(),
            },
        });
        assert_eq!(view.available.amount, Decimal::new(1000, 0));
        assert_eq!(view.pending.amount, Decimal::new(500, 0));

        view.update(&EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 3,
            payload: LedgerEvent::LedgerUpdated {
                amount: Money::new(Decimal::new(500, 0), Currency::USD),
                transaction_id: "tx1".to_string(),
                transaction_type: "credit_release".to_string(),
                available_delta: Money::new(Decimal::new(500, 0), Currency::USD),
                pending_delta: Money::new(Decimal::new(-500, 0), Currency::USD),
                base_event: base_event.clone(),
            },
        });
        assert_eq!(view.available.amount, Decimal::new(1500, 0));
        assert_eq!(view.pending.amount, Decimal::ZERO);
        assert_eq!(view.current.amount, Decimal::new(1500, 0));

        // DebitHold $200
        view.update(&EventEnvelope {
            aggregate_id: "ledger1".to_string(),
            metadata: Default::default(),
            sequence: 4,
            payload: LedgerEvent::LedgerUpdated {
                amount: Money::new(Decimal::new(200, 0), Currency::USD),
                transaction_id: "tx2".to_string(),
                transaction_type: "debit_hold".to_string(),
                available_delta: Money::new(Decimal::new(-200, 0), Currency::USD),
                pending_delta: Money::new(Decimal::new(200, 0), Currency::USD),
                base_event: base_event.clone(),
            },
        });
        assert_eq!(view.available.amount, Decimal::new(1300, 0));
        assert_eq!(view.pending.amount, Decimal::new(200, 0));
        assert_eq!(view.current.amount, Decimal::new(1500, 0));
    }

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
