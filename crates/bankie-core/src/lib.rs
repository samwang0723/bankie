pub mod auth;
pub mod command;
pub mod common;
pub mod configs;
pub mod domain;
pub mod event_sourcing;
pub mod house_account;
pub mod interest;
pub mod job;
pub mod report;
pub mod repository;
pub mod route;
pub mod service;
pub mod state;

use repository::pools::DbPools;
use state::ApplicationState;
use std::sync::Arc;

/// Shared application state wrapped in `Arc` for thread-safe sharing across handlers and jobs.
pub type SharedState = Arc<ApplicationState<DbPools>>;
