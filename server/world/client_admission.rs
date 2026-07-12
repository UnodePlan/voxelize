use std::sync::Arc;

use crate::ConnectionPrincipal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientAttachKind {
    Join,
    Rebind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientAttachRequest {
    pub kind: ClientAttachKind,
    pub world_name: String,
    pub client_id: String,
    pub attach_attempt_id: String,
    pub principal: Option<ConnectionPrincipal>,
}

pub(crate) type ClientAttachGuard =
    Arc<dyn Fn(&ClientAttachRequest) -> bool + Send + Sync + 'static>;
