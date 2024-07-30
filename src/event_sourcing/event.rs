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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

impl PartialEq for BaseEvent {
    fn eq(&self, other: &Self) -> bool {
        if self.aggregate_id != other.aggregate_id || self.parent_id != other.parent_id {
            return false;
        }

        // Parse the created_at strings into DateTime objects
        let self_created_at = DateTime::parse_from_rfc3339(&self.created_at).ok();
        let other_created_at = DateTime::parse_from_rfc3339(&other.created_at).ok();

        match (self_created_at, other_created_at) {
            (Some(self_dt), Some(other_dt)) => {
                // Compare timestamps, allowing for a small difference (e.g., 1 second)
                (self_dt - other_dt).abs().num_seconds() <= 1
            }
            _ => self.created_at == other.created_at, // Fall back to string comparison if parsing fails
        }
    }
}
