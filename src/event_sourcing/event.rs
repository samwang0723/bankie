use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub trait Event {
    // GetAggregateID returns event's aggregate id.
    fn get_aggregate_id(&self) -> String;

    // SetAggregateID changes event's aggregate id.
    fn set_aggregate_id(&mut self, id: Uuid);

    // GetParentID returns event's parent id
    fn get_parent_id(&self) -> String;

    // SetParentID changes event's parent id
    fn set_parent_id(&mut self, id: Uuid);

    // GetCreatedAt returns event's create time
    fn get_created_at(&self) -> String;

    // SetCreatedAt changes event's create time
    fn set_created_at(&mut self, created_at: DateTime<Utc>);
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BaseEvent {
    created_at: String,
    aggregate_id: String,
    parent_id: String,
}

impl Event for BaseEvent {
    fn get_aggregate_id(&self) -> String {
        self.aggregate_id.clone()
    }

    fn set_aggregate_id(&mut self, id: Uuid) {
        self.aggregate_id = id.to_string();
    }

    fn get_parent_id(&self) -> String {
        self.parent_id.clone()
    }

    fn set_parent_id(&mut self, id: Uuid) {
        self.parent_id = id.to_string();
    }

    fn get_created_at(&self) -> String {
        self.created_at.clone()
    }

    fn set_created_at(&mut self, created_at: DateTime<Utc>) {
        self.created_at = created_at.to_rfc3339();
    }
}
