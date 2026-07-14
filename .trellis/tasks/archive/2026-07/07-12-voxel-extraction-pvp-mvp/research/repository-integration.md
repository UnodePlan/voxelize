# 仓库接入与核心边界研究

## 结论

体素撤离 PVP 应作为独立应用构建，Voxelize 核心继续负责连接、World Actor、ECS、区块、物理和通用消息。账号、钱包、比赛、背包、战斗、撤离和永久资产均属于应用层。核心只增加其他游戏也能复用的鉴权、原子加入、严格请求策略、连接重绑和 World 生命周期能力。

建议目录：

```text
apps/
  extraction-server/   # 独立 Cargo manifest，path 依赖根 voxelize crate
  extraction-client/   # 独立 Vite/TypeScript pnpm 包
  extraction-e2e/      # 轻量协议客户端与浏览器闭环测试
```

独立服务端 manifest 避免把 SQLx、SIWE、钱包和业务依赖加入根引擎 crate。客户端通过在 `pnpm-workspace.yaml` 增加 `apps/*` 接入现有 pnpm 工作区。

## 可直接复用

- 玩家创建时已有 `PositionComp`、`DirectionComp`、`RigidBodyComp`、`InteractorComp`、`CollisionsComp` 和 `MetadataComp`（`server/world/mod.rs:929-960`）。
- `set_client_modifier` 可为玩家挂载应用组件，`set_method_handle` 可接收与连接身份绑定的操作意图（`server/world/mod.rs:1120-1148`）。
- `MetadataComp` 能序列化自定义组件，但私有背包和自身生命应通过 `ClientFilter::Direct` 发送，不能广播给所有 Peer（`server/world/components/metadata.rs:31-71`）。
- 服务端体素射线 `trace` 可做完整方块遮挡，10 人近战实体命中可线性遍历玩家 AABB（`server/world/physics/raycast.rs:133-156`）。
- `ItemRegistry`、`SlotContent` 和客户端 Item renderer/ItemSlots 可复用为静态目录、槽位 DTO 和显示基础（`server/world/items/registry.rs:7-108`，`server/world/items/slot.rs:5-155`，`packages/core/src/libs/item-slots.ts:46-100`）。
- 客户端已有第一人称手臂挥动动画和 Method/Event 网络基础（`packages/core/src/libs/arm.ts:218-235`，`packages/core/src/core/method.ts:21-47`）。
- `ChunkStage`、生成 Pipeline 和 seed 可用于版本化地图生成；现有 JSON 实体/区块保存不得作为永久资产账本。

## 必须新增的核心通用能力

### 可插拔 HTTP 与 WebSocket 鉴权

`Voxelize::run` 当前硬编码 permissive CORS、`/ws/` 和 `/info`，并用共享 secret 与查询参数 `client_id` 建立身份（`server/lib.rs:48-103`、`server/lib.rs:223-269`）。需要：

- 支持应用注册 HTTP routes/app data；
- 支持异步 `ConnectionAuthenticator`，从 HttpOnly 会话 cookie 解析不可伪造的 `ConnectionPrincipal`；
- public strict 模式禁止客户端指定身份；
- legacy 模式保留现有 Demo 行为，避免破坏上游示例；
- CORS 使用配置化 origin allowlist，日志不得输出 secret、cookie 或签名。

### 原子 Join

Server 当前在 World 实际处理 Join 前就从 `lost_sessions` 移除连接并写入 `connections`（`server/server/mod.rs:295-323`）；`ClientJoinRequest` 又没有返回值（`server/world/mod.rs:379-386`）。需要：

- World 在同一临界区校验账号席位、比赛阶段、名单和容量；
- `ClientJoinRequest` 返回明确 `Result`；
- 只有 World 接纳成功后 Server 才提交连接表变更；
- 并发第 10/11 人必须确定性得到 10 个成功、1 个拒绝；
- `max_clients` 不再只是未执行配置。

### 连接 detach/rebind

当前断线会立即移除刚体、碰撞、ClientFlag 并删除玩家实体（`server/world/mod.rs:1033-1107`）。比赛需要把“网络 sender 脱离”“60 秒内同账号重绑”“最终 despawn”拆成三个通用操作。

### 动态 World 生命周期

Server 只有 `add_world/get_world`，没有安全 remove/stop；动态 World 也没有完整 prepare/preload 管理（`server/server/mod.rs:235-287`，`server/lib.rs:226-229`）。需要：

- prepare/preload 成功后才开放 Join；
- stop/remove 清理 Actor、`worlds`、`connections`、`pending_world_ticks`；
- 重复 remove 幂等；
- 连续多局与并发多局不残留 World；
- 禁止根据不可信路径递归删除存档目录，比赛 World 使用 `saving(false)`。

## 必须硬化的请求边界

- `MessageType::from_i32(...).unwrap()` 对非法枚举会 panic（`server/world/mod.rs:1196-1224`）。
- Method/Event 当前先用小写检查、再用原始名称 `get(...).unwrap()`，大小写变体可触发 panic（`server/world/mod.rs:1845-1896`）。
- Join/Action/Transport JSON 仍有 `expect`，公网输入必须返回受控错误（`server/server/mod.rs:295-340`，`server/world/mod.rs:1208-1218`）。
- raw `UPDATE` 可直接修改任意方块，bulk 数组长度不一致还会越界；strict World 必须禁用（`server/world/mod.rs:1811-1843`）。
- 未注册客户端 Event 默认会被转发；strict World 必须使用入站 allowlist，服务端专属事件一律拒绝。
- 默认 parser 直接接受客户端位置、flying 和 ghost；PVP 需要有限数、速度、体素 sweep、精确 `300 x 300` 边界和玩家终态校验（`server/world/mod.rs:148-215`）。
- `vox-builtin:set-time`、`update-block-entity` 等管理型 Method 在 strict World 中必须关闭或需要能力校验。

无条件的解析、防 panic 和数组边界修复应保留在核心。会改变 Demo 行为的权限限制通过 opt-in strict policy 启用。

## 协议策略

玩法优先复用 Method/Event，使用版本化 JSON envelope，不修改 Protobuf：

- Rust 在单一模块拥有 serde DTO 与解码；
- TypeScript 在单一模块拥有 runtime decoder 和 reducer；
- 两端共享成功/失败 JSON fixture；
- 客户端只提交 sequence、action、slot、voxel 等意图，不提交账号、目标、伤害、资源类型或数量；
- 服务端私有状态只直发本人，附近玩家仅接收无资产含义的表现事件。

只有现有 envelope 无法表达且有明确收益时才修改 `messages.proto`；若修改，Rust/TS 生成物和兼容测试必须同一提交完成。

## 现有验证命令

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo test --lib --tests
pnpm test
pnpm --filter @voxelize/core build
pnpm --filter client build
git diff --check
```

当前仓库没有账号、数据库、PVP 或多客户端闭环测试，也没有可直接复用的可视化管理后台。
