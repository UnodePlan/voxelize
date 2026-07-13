# World Actor 文件规模说明

## 当前原因

`server/world/mod.rs` 是上游已有的 World/SyncWorld 主模块，当前超过 2300 行；阶段 3 只增加 generation 准备消息、客户端准入 guard 接线和短 Handler，阶段 5 增加两个通用组合入口 `extend_dispatcher` 与 `add_client_modifier`，阶段 6 增加单一命名的 `ChunkUpdating` 前置系统安装点，阶段 7 增加单一命名的 `Broadcast` 前置系统安装点。具体玩法仍位于 extraction 应用；客户端校验位于 `client_admission.rs`、`client_lifecycle.rs` 与 `lifecycle.rs`。

`client_lifecycle.rs` 当前为 328 行，超过 300 行软上限但低于 500 行硬上限。它集中维护 Join、Detach、Rebind、条件 Despawn 对同一 `Clients` 资源的原子转换；阶段 3 新增的 generation 与 attach attempt 比较用于防止旧回调删除新租约，相关 Actor 测试已放在独立测试文件。

## 约束

- World 客户端生命周期必须在 Actor 邮箱内串行修改 `Clients`，不能为拆文件引入第二套状态或异步旁路。
- Rebind 失败必须恢复 detached lease；条件清理必须同时比较 generation 与 attach attempt。
- `mod.rs` 不继续承载应用层匹配、背包、战斗或结算逻辑，PVP 业务应留在 extraction 应用模块。
- dispatcher 扩展只能组合当前 factory 并使已构建缓存失效，不能复制默认系统链；client modifier 必须保留“已有逻辑先执行、新逻辑后执行”的顺序。
- `ChunkUpdating` 与 `Broadcast` 前置安装点只允许默认 dispatcher 使用，且各自只能安装一次；系统名冲突、自定义 dispatcher 和重复安装都必须显式失败。

## 后续计划

- 下次扩展客户端生命周期时，把 Join 与 Detach/Rebind Handler 按资源所有权拆为两个子模块，保持公共消息类型在 `lifecycle.rs`。
- 继续把 World 准备、停止和动态移除逻辑放在专责模块，不向 `mod.rs` 堆叠实现。
- 拆分必须以现有 generation/attempt ABA、重复回调和 stale World 测试为门禁，不做只为行数的机械移动。
