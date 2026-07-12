use std::sync::Arc;

use crate::ConnectionPrincipal;

/// 为已认证连接解析应用层分配的 World 公开客户端 ID。
///
/// 解析在 Server Actor 内同步执行，实现必须快速返回且不能回调同一个 Actor。
/// 配置解析器后返回 `None` 会拒绝当前 Join，且不会向 World 发送加入请求。
pub trait AuthenticatedClientIdResolver: Send + Sync {
    fn resolve_client_id(
        &self,
        world_name: &str,
        principal: &ConnectionPrincipal,
    ) -> Option<String>;
}

impl<F> AuthenticatedClientIdResolver for F
where
    F: Fn(&str, &ConnectionPrincipal) -> Option<String> + Send + Sync,
{
    fn resolve_client_id(
        &self,
        world_name: &str,
        principal: &ConnectionPrincipal,
    ) -> Option<String> {
        self(world_name, principal)
    }
}

pub(crate) type SharedAuthenticatedClientIdResolver = Arc<dyn AuthenticatedClientIdResolver>;
