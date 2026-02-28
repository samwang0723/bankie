use std::sync::Arc;

use crate::repo::api_key::ApiKeyRepository;
use crate::repo::dashboard::DashboardRepository;
use crate::repo::member::MemberRepository;
use crate::repo::org::OrgRepository;

pub struct PortalState {
    pub org_repo: Arc<dyn OrgRepository>,
    pub member_repo: Arc<dyn MemberRepository>,
    pub api_key_repo: Arc<dyn ApiKeyRepository>,
    pub dashboard_repo: Arc<dyn DashboardRepository>,
    pub jwt_secret: String,
}
