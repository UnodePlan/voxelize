# 体素撤离 PVP MVP 实施计划

## 实施边界

- 业务代码放入 `apps/extraction-server`、`apps/extraction-client`、`apps/extraction-e2e`；引擎核心只接收可复用的连接、Join、World 生命周期和 strict policy 扩展。
- 服务端应用使用独立 Cargo manifest，通过 path 依赖根 `voxelize`，避免 SQLx/SIWE 污染引擎 crate；客户端以 `apps/*` 加入 pnpm workspace。
- 游戏协议优先使用版本化 JSON Method/Event；Rust serde DTO、TypeScript runtime decoder 和 fixture 各只有一个真相来源。
- 所有比赛时间依赖可注入 `Clock` 和绝对 deadline；数据库只保存 UTC 审计时间，World tick 不等待数据库 I/O。
- PVP World 使用 `saving(false)`；永久资产只写 PostgreSQL，不导入旧 world/entity JSON。
- legacy Demo 保持默认兼容；PVP 显式启用 strict/authoritative policy，每阶段均回归现有引擎与 Demo。
- 新增 Reown、SQLx、SIWE 等依赖以及对任何非一次性数据库执行 migration 前，必须按危险操作规则另行确认。

## 阶段 0：应用骨架与契约门禁

- [x] 创建三个独立应用、环境变量示例、健康检查和最小测试入口，不改造现有 Demo 为产品客户端。
- [x] 定义稳定 `dirt/gold/diamond` 键、显式运行时 ID、配置版本、错误码、消息 envelope 和共享 fixture。
- [x] 定义 `Clock`、ID、seed 与 repository trait，所有领域测试可注入替身。
- [x] 建立 Rust 测试、客户端 Vitest、跨端 fixture 和最小 CI；密钥、Project ID 私有配置和 DSN 不提交。
- [x] 解除 `engine` feature 的 Rust MSRV 阻塞并加入 CI；调整 Rust 工具链或核心传递依赖前需另行危险操作确认。

实现记录（2026-07-12）：Rust 工具链已固定为 1.93.0，根 crate 与 extraction server 均声明 MSRV 1.93；聚合构建、聚合测试和 CI 均以 `engine` feature 真实链接根 `voxelize`。CI 额外固定 protoc 25.3，覆盖所有引擎构建输入，并让必需 pnpm package filter 在空匹配时失败。锁定依赖下的 engine check/test、根 `cargo check --all-targets` 和根 Rust 测试均通过，Cargo lock 未漂移。现有 Demo 源码未改动；根 Core 构建还受缺少 WASM 生成物影响，示例客户端仍存在与本次差异无关的 TypeScript 基线错误，因此不把完整 Demo 构建声称为已通过。根引擎已有警告仍保留为基线，应用默认骨架继续执行 Clippy 零警告门禁。

验收：空应用可编译启动，Rust/TypeScript 对同一 fixture 解析一致，现有 Demo 不变。

回滚：骨架与 workspace 注册独立提交，可整体移除而不影响引擎。

## 阶段 1：Voxelize 核心信任边界

- [ ] 抽出可插拔 HTTP 配置与异步 `ConnectionAuthenticator`，principal 替代 public 模式的查询参数 `client_id`。
- [ ] 删除 secret 明文日志，配置精确 CORS/Origin、请求大小和有界队列；生产认证失败关闭，不回退 guest。
- [ ] 将 World Join 改为有返回值的原子操作，World 接纳成功后 Server 才提交连接表；覆盖 10/11 人竞态。
- [ ] 增加 opt-in strict policy：禁 raw voxel UPDATE、未知 Event 转发、客户端 flying/ghost 和未授权管理 Method。
- [ ] 修复非法 MessageType、畸形 JSON、大小写 handler key、bulk 长度不一致等公网 panic。
- [ ] 拆分 detach/rebind/despawn，并增加 prepare/preload/stop/remove World 的幂等生命周期。
- [ ] 新逻辑拆入小模块；`server/server/mod.rs`、`server/world/mod.rs` 只保留必要接线。

验收：无需账号或玩法即可通过 Actor 测试证明 principal、原子 Join、strict 拒绝、重绑、World 销毁及 legacy 兼容。

回滚：strict policy 默认关闭；核心解析和防 panic 修复永久保留。

## 阶段 2：PostgreSQL、SIWE 与会话

- [ ] 先做 `signinwithethereum 0.8.x + alloy` 在项目工具链上的 EOA/EIP-1271/EIP-6492 编译验证。
- [ ] 创建 additive migrations：账号/钱包、nonce/session、比赛/参与者、带 digest/config version 的 settlement/items、warehouse、ledger。
- [ ] 实现 nonce 5 分钟 TTL、条件消费和 SIWE 全字段验证；Mainnet chain ID 固定为 1。
- [ ] 创建只存 hash 的安全 HttpOnly Cookie session；WS 从 cookie 得到 principal，不接受客户端账号字段。
- [ ] 实现 `/api/auth/siwe/nonce`、`/api/auth/siwe/verify`、`/api/auth/session`、`/api/auth/logout`、`/api/warehouse` 和 FIFO queue API。
- [ ] RPC 验证设置超时与并发上限；失败不得跳过验证，已有会话与比赛不依赖 RPC。

验收：API 测试完成 nonce -> SIWE -> session -> principal；错误链/domain/URI/签名、重复 nonce 均拒绝；RPC 故障不影响已有会话，官方客户端换地址/网络触发 logout 后旧 Cookie 被拒绝。

回滚：migration 只增不删；认证 feature flag 关闭时停止登录/匹配，不恢复伪造身份入口。

## 阶段 3：恰好 10 人比赛与 World 生命周期

- [ ] 实现 FIFO queue、每账号一个非终态席位、9 人 Waiting、第 10 人原子冻结名单、第 11 人拒绝；终态后释放互斥以允许重新排队。
- [ ] 实现 `Waiting -> Preparing -> Active -> ExtractionOpen -> Settling -> Finished|Aborted` 与参与者单向状态。
- [ ] Preparing 断线时中止未开局批次、清理部分 World，并按原排队时间恢复其余在线玩家，不创建成绩或资产。
- [ ] 创建独立内存 World，底层 320 范围、玩法精确 300 边界；写入 seed 和三类版本号。
- [ ] 开局记录 `+8m`、`+12m` 绝对 deadline；全部终态可提前进入 Settling。
- [ ] Waiting 断线离队；Active 断线 detach，60 秒内同账号 rebind，超时只死亡一次。
- [ ] Finished/Aborted 冻结写入并清理 World、连接、pending tick 和所有局内资源。
- [ ] 用不同永久仓库存量的账号验证固定初始装备/属性完全一致，并拒绝任何客户端仓库配装字段。

验收：fake clock + 11 个轻量 Actor 客户端覆盖 9/10/11、Preparing 断线回队、迟到加入、终态重新排队、固定公平入场、60 秒重连和连续多局无泄漏。

回滚：先支持单进程房间，不引入 Redis 或跨进程 World。

## 阶段 4：确定性地图与版本化资源

- [ ] 注册稳定资源/装备定义并实现只依赖 seed/version/config 的 ChunkStage。
- [ ] 泥土全图常见；黄金中层至少两个热点；钻石更稀少、深层、中心偏置且至少两个热点。
- [ ] 配置密度、矿脉、深度、出生点、撤离候选和首版 `1/10/100` 统计权重；已被比赛引用的版本不可原地修改或追溯重估。
- [ ] 固定 seed 测试 fingerprint、热点连通分量、数量/深度区间和不同 seed 差异。

验收：相同输入地图完全复现，新比赛不继承上一局地形。

回滚：调参只新增版本；旧版本保留复现能力。

## 阶段 5：权威背包、掉落与自动拾取

- [ ] 实现 12 格私有 `MatchInventory`、固定装备、64 堆叠和单调 revision。
- [ ] 所有增减走领域方法，按已有堆叠/空格顺序返回 accepted/remainder。
- [ ] 实现确定性 Drop ID、`PendingDropQueue`、世界 Loot、空间桶合并和比赛结束清理；掉落不按 TTL 删除。
- [ ] 自动拾取在服务端按距离/seat 稳定排序，原子更新背包和掉落剩余。
- [ ] 整组丢弃校验 sequence/slot/revision，丢弃者屏蔽 2 秒；不支持拆分或固定装备。

验收：64/65、满包、部分接纳、双人争抢、整组丢弃及 `背包 + pending + 地面` 守恒测试通过。

回滚：发现复制风险时先关闭新匹配，不用负余额或删流水补偿。

## 阶段 6：服务端权威挖掘

- [ ] 客户端只发送 start/maintain/cancel 和 sequence，服务端校验状态、阶段、工具、边界、距离、视线和方块。
- [ ] fake clock 驱动泥土/黄金/钻石 `0.5/1.5/3s`；所有取消条件从零重置。
- [ ] 完成时先用 `HarvestedVoxelSet` claim，再写 AIR、固定产出 1、入包或 pending drop。
- [ ] `MiningResolution -> ChunkUpdating` 顺序固定；strict World 始终禁用客户端经济方块 raw UPDATE。
- [ ] 客户端进度只显示服务端 revision 状态。

验收：同体素重复/乱序/并发完成最多产出 1；满包仍破坏一次并留下 1 个公共资源。

回滚：不可为兼容重新开放 raw UPDATE。

## 阶段 7：近战、生命、死亡与结果

- [ ] 实现 20 半心、每次伤害 2、0.6 秒冷却、3 格距离、完整方块遮挡和无回血。
- [ ] 攻击只提交 sequence/武器槽；服务端 ray-AABB 选择最近合法目标，落空也消费冷却。
- [ ] 固定同 tick 优先级，死亡早于拾取和撤离；`Alive -> Dead` CAS 只执行一次。
- [ ] 死亡冻结背包并转入唯一 pending loot，固定装备不掉，记录击杀者和结果统计。
- [ ] 断线角色可被攻击；窗口内被杀和超时不能双掉落。死亡结果后离开原 World，不接收观战数据。

验收：第 10 次合法命中死亡一次；冷却、距离、遮挡、伪造目标/伤害、旧 sequence 和重连攻击均拒绝；team/role 元数据不产生友军豁免，重连不回血而新比赛恢复 20 半心。

回滚：战斗入口 feature flag 可关闭，但资源守恒与核心 hardening 保留。

## 阶段 8：撤离、结算与异常恢复

- [ ] 第 8 分钟才发布唯一撤离区；服务端累计连续 8 秒，离区/死亡/断线中断；仅 `qualified_at <= hard_deadline` 进入待结算。
- [ ] 达标后冻结 participant 和背包，生成规范快照、digest、config version 与 `extract:v1:{match}:{account}` 幂等键。
- [ ] 异步事务原子写 settlement/items、ledger、warehouse 和 participant Extracted；World tick 不等待 SQL。
- [ ] commit 后才确认；未知结果先查询，commit 前失败为 0，commit 后响应丢失仍为 1。
- [ ] 实现硬截止后 30 秒写入宽限期；期满不再发起写入，仅保留有界只读 reconciliation，并允许清理玩法 World。
- [ ] 启动时把未结束比赛标 Aborted，保留已提交收益，其他局内收益作废。
- [ ] 实现仅允许本人访问的 `/api/matches/{match_id}/result` 与 `/api/matches/latest-result`，覆盖 PendingReconciliation/Extracted/Aborted。
- [ ] 仓库只读展示数量和 settlement 聚合统计，不提供消费/交易/属性入口。

验收：fake clock 覆盖 8m/8s/12m/30s 边界；截止前达标且截止后 commit 仍成功，截止后达标失败；故障注入覆盖 commit 前、回滚、commit 后响应前、宽限期后只读核对和重启核对。

回滚：不删除/反向修改 settlement 与 ledger；暂停新匹配并只读 reconciliation。

## 阶段 9：产品客户端与 Reown/UI

- [ ] 实现实际登录/大厅/比赛/结果/仓库首屏流程，不制作营销落地页。
- [ ] 集成 AppKit Ethers adapter、Mainnet 和 SIWE；连接钱包但未认证不能查仓库或排队。
- [ ] HUD 显示稳定 10 心/半心、12 格背包、固定装备、阶段倒计时、挖掘和撤离进度。
- [ ] 死亡结果显示击杀者/存活/资源得失；撤离仅在 commit 后显示永久入账；重新登录可恢复待核对或异常比赛结果。
- [ ] 重连恢复全量 revision；换钱包/网络调用服务端 logout、关闭 WS 并清理控制状态。
- [ ] 单一 typed decoder/reducer 拥有状态；测试 bridge 只进入测试构建。

验收：Vitest 覆盖钱包/换链注销、revision、心形 HUD、待核对/异常结果；模拟 EIP-1193 provider 断言闭环中从不调用交易方法；生产 build 不含测试后门。

回滚：UI 与版本化契约可独立回滚，环境配置不写死。

## 阶段 10：内部只读运维

- [ ] 提供账号、比赛、participant、settlement、warehouse 和 ledger 的固定只读查询。
- [ ] 使用独立只读角色、内网/CLI 边界、分页、限流、超时和脱敏访问日志。
- [ ] 拒绝匿名、普通玩家、写请求、任意 SQL、余额修改和 settlement 重放。

验收：授权查询成功，所有写入尝试失败且访问有不泄密的审计记录。

回滚：运维入口可整体关闭，不影响游戏。

## 阶段 11：E2E、观测与发布门禁

- [ ] 轻量协议客户端覆盖容量与竞态；每次变更由 2 个 Playwright 浏览器 + 8 个轻量协议客户端组成合法 10 人闭环，10 浏览器完整场景用于 nightly/发布前。
- [ ] 完整闭环：10 人 -> 挖掘 -> 10 次近战 -> 唯一掉落 -> 自动拾取 -> 8 分钟开放 -> 8 秒撤离 -> 一次入仓。
- [ ] 覆盖第 11 人、迟到加入、断线重连/被杀/超时、满包、重复消息、DB 故障和 Aborted。
- [ ] 连续多局检查 worlds、connections、pending ticks、掉落实体与内存不线性增长。
- [ ] 结构化记录阶段、拒绝、死亡、settlement 和恢复结果，严禁记录签名/token/secret。
- [ ] Playwright 检查桌面/移动 HUD 不重叠、canvas 非空、场景移动且资产可见。

验收：受控环境核心闭环全绿，故障不复制永久资产，连续多局无泄漏。

回滚：测试 bridge 编译期隔离；发布失败只回滚应用，不破坏账本。

## 验证命令

当前可执行的根项目回归：

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo test --lib --tests
pnpm test
pnpm --filter @voxelize/core build
pnpm --filter client build
git diff --check
```

应用骨架落地后必须提供并执行：

```bash
cargo fmt --manifest-path apps/extraction-server/Cargo.toml -- --check
cargo clippy --manifest-path apps/extraction-server/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path apps/extraction-server/Cargo.toml --all-targets --features engine --locked
TEST_DATABASE_URL="$DISPOSABLE_TEST_DATABASE_URL" cargo test \
  --manifest-path apps/extraction-server/Cargo.toml --features db-tests -- --test-threads=1
pnpm --filter @voxelize/extraction-client test
pnpm --filter @voxelize/extraction-client build
pnpm --filter @voxelize/extraction-e2e test:actor
pnpm --filter @voxelize/extraction-e2e test:browser:smoke
pnpm --filter @voxelize/extraction-e2e test:browser:10p
```

`sqlx migrate run` 只允许用于一次性测试库或另行确认的目标数据库；新命令在对应 package scripts/Cargo features 落地前只是规划契约，不能声称已运行。

## 高风险文件与回滚点

- `server/world/mod.rs`：只接线/兼容改动；重点回归 parser、dispatcher、UPDATE/Event 和 client lifecycle。
- `server/server/mod.rs`：原子 Join、身份、连接表和 World 删除必须有 Actor 竞态测试。
- `server/lib.rs`：握手、认证、CORS 影响全部客户端，strict/legacy 都测。
- `packages/core/src/core/network/index.ts`：重连/重新 Join 易重复连接，使用 fake WebSocket 测试。
- `messages.proto`：非必要不改；如改必须同时生成 Rust/TS 并做 fixture 兼容测试。
- Cargo/pnpm manifests 与 locks：依赖树单独审查，不升级无关包。
- `apps/extraction-server/migrations`：只做 additive；非测试库执行前重新确认。

## 开始实现前门禁

- [ ] 用户审核并批准 `prd.md`、`design.md` 和本文件。
- [ ] PRD 已完成无损收敛，无重复事实或已解决开放问题。
- [ ] `implement.jsonl`、`check.jsonl` 均有真实 research/spec 条目并通过校验。
- [ ] 用户对新增依赖和实际数据库 migration 的危险操作范围另行确认。
- [ ] 以上完成后才执行 `task.py start`，当前阶段不得开始业务实现。
