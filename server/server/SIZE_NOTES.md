# Server Actor 文件规模说明

## 当前原因

`server/server/mod.rs` 是上游已有的 Actix `Server` Actor 主模块，当前同时承载 Actor 状态、公开消息类型、Builder 接线和短 Handler。阶段 1 已把连接状态、Join、客户端请求、WebSocket 与 World 生命周期拆到同目录专责模块，但主文件仍超过 500 行。

阶段 2 只在主模块增加会话撤销消息、Leave 过渡状态接线和短 Handler。认证、Cookie、数据库、SIWE、HTTP 路由及绝大多数测试均位于应用层或现有小模块，没有继续向主文件堆叠业务实现。

阶段 3 增加动态 World generation、公开玩家 ID 解析器和带 attach attempt 的 Join/Rebind 生命周期。身份解析、观察事件、detach/despawn 和 World 准备逻辑已分别拆到 `authenticated_client_id.rs`、`connection_lifecycle.rs`、`detached_lifecycle.rs` 与 `world_lifecycle.rs`；主文件仍保留需要同时核对多组私有连接表的 Rebind Actor 回调，当前为 1203 行。

## 约束

- Actor 字段和 Handler 需要访问同一组私有连接状态；在本阶段整体搬移会同时改变模块可见性、Actor 消息注册和竞态测试范围。
- 会话撤销必须复用 token-aware `Disconnect`、pending Join、rebind 和 World 移除路径，不能另建平行状态机。
- Rebind 提交同时核对 connection token、World generation、principal、attach attempt 和断线/销毁标志；在没有先稳定连接状态所有权边界前机械搬移会增加跨模块可见性和 ABA 回归风险。
- 本文件记录的是既有超限文件的受控例外，不允许后续功能默认继续写入 `mod.rs`。

## 后续计划

- 新增连接或认证生命周期逻辑优先放入 `connections.rs`、`client_requests.rs`、`world_lifecycle.rs` 或新的专责模块。
- 当再次修改主 Actor 状态布局时，将认证会话关闭消息与 Handler 成组拆入 `authenticated_sessions.rs`，并把 Rebind 回调连同 pending 状态所有权拆入专责模块，以现有 Actor 竞态测试作为迁移门禁。
- 不为满足行数进行机械拆分；拆分必须同时减少状态耦合，并保持消息顺序和断线幂等语义。
