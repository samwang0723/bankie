pub mod api_key;
pub mod auth;
pub mod dashboard;
pub mod data_proxy;
pub mod logs;
pub mod member;
pub mod org;
pub mod webhook;

use std::sync::Arc;

use axum::{middleware as axum_mw, Router};

use crate::middleware::session::session_auth;
use crate::state::PortalState;

/// Build the complete portal API router.
pub fn portal_router(state: Arc<PortalState>) -> Router {
    // Public routes (no session required)
    let public = Router::new()
        .merge(auth::auth_routes())
        .with_state(Arc::clone(&state));

    // Protected routes (session auth required)
    let protected = Router::new()
        .merge(org::org_routes())
        .merge(api_key::api_key_routes())
        .merge(dashboard::dashboard_routes())
        .merge(data_proxy::data_proxy_routes())
        .merge(member::member_routes())
        .merge(webhook::webhook_routes())
        .merge(logs::logs_routes())
        .route_layer(axum_mw::from_fn_with_state(
            Arc::clone(&state),
            session_auth,
        ))
        .with_state(Arc::clone(&state));

    Router::new()
        .nest("/portal/v1", public)
        .nest("/portal/v1", protected)
}
