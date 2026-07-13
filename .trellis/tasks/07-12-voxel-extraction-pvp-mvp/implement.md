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

- [x] 抽出可插拔 HTTP 配置与异步 `ConnectionAuthenticator`，principal 替代 public 模式的查询参数 `client_id`。
- [x] 删除 secret 明文日志，配置精确 CORS/Origin、请求大小和有界队列；生产认证失败关闭，不回退 guest。
- [x] 将 World Join 改为有返回值的原子操作，World 接纳成功后 Server 才提交连接表；覆盖 10/11 人竞态。
- [x] 增加 opt-in strict policy：禁 raw voxel UPDATE、未知 Event 转发、客户端 flying/ghost 和未授权管理 Method。
- [x] 修复非法 MessageType、畸形 JSON、大小写 handler key、bulk 长度不一致等公网 panic。
- [x] 拆分 detach/rebind/despawn，并增加 prepare/preload/stop/remove World 的幂等生命周期。
- [x] 新逻辑拆入小模块；`server/server/mod.rs`、`server/world/mod.rs` 只保留必要接线。

实现记录（2026-07-12）：新增 `HttpConfig` 的 legacy/public-strict 双模式、异步 `ConnectionAuthenticator`、精确 Origin/CORS 门禁、HTTP/WS 大小限制、有界出站与 World 请求队列，以及认证/消息超时；public 模式忽略查询参数身份并使用认证 principal 与服务端生成的单局公开玩家 ID。World Join 以 attempt/generation 租约原子接纳，World 返回 receipt 后 Server 才提交连接路由；离开、取消 Join、detach、rebind、despawn 分离并保持 Actor 邮箱顺序。World 生命周期完成 `Created -> Preparing -> Ready -> Stopping -> Stopped`、动态 AddWorld、幂等 RemoveWorld 和旧 Addr 关闭；strict policy 默认拒绝 raw UPDATE、移动特权、命令和未列入白名单的 Method/Event，同时修复非法枚举、JSON、bulk 和 handler key 的 panic 路径。客户端仅在显式 Leave 后允许下一局 INIT 替换公开 ID，同一席位重绑仍拒绝 ID 漂移。

验证记录：`cargo test --lib --tests` 53/53、`cargo check --all-targets`、extraction-server engine 测试 9/9、extraction-server 格式与 Clippy 零警告、根 `pnpm test` 19/19、extraction-client 12/12 及生产构建、extraction E2E Actor 2/2、全部变更 Rust 文件的定向 rustfmt、Prettier 与 `git diff --check` 均通过。未修改 Cargo/pnpm manifest 或锁文件，新增代码未发现 secret/debug 输出。全仓 `cargo fmt --all -- --check` 仍被既有 mesher、灯光测试等格式差异阻塞；`@voxelize/core` build/types 仍因仓库缺少 `@voxelize/wasm-mesher` 生成物失败；旧 Demo 客户端 build 仍因其 TypeScript 工具链不能解析当前 `@types/d3-dispatch` 的 `const` 类型参数失败，均非本阶段差异。

文件规模说明：新增生产模块均低于 300 行；`server/http.rs` 与 `server/server/websocket.rs` 的生产段分别为 297/270 行，超出部分仅为同文件测试。既有 `server/server/mod.rs` 与 `server/world/mod.rs` 在上游已超过 500 行，本阶段已将 Join、请求转发、连接状态、WebSocket、World 生命周期和客户端生命周期拆至小模块，保留文件只承载原有主体与必要接线；继续机械拆分会扩大本阶段回归范围，后续修改仍须优先向现有小模块收敛。

验收：无需账号或玩法即可通过 Actor 测试证明 principal、原子 Join、strict 拒绝、重绑、World 销毁及 legacy 兼容。

回滚：strict policy 默认关闭；核心解析和防 panic 修复永久保留。

## 阶段 2：PostgreSQL、SIWE 与会话

- [x] 先做 `signinwithethereum 0.8.x + alloy` 在项目工具链上的 EOA/EIP-1271/EIP-6492 编译验证。
- [x] 创建 additive migrations：账号/钱包、nonce/session、比赛/参与者、带 digest/config version 的 settlement/items、warehouse、ledger。
- [x] 实现 nonce 5 分钟 TTL、条件消费和 SIWE 全字段验证；Mainnet chain ID 固定为 1。
- [x] 创建只存 hash 的安全 HttpOnly Cookie session；WS 从 cookie 得到 principal，不接受客户端账号字段。
- [x] 实现 `/api/auth/siwe/nonce`、`/api/auth/siwe/verify`、`/api/auth/session`、`/api/auth/logout`、`/api/warehouse` 和 FIFO queue API。
- [x] RPC 验证设置超时与并发上限；失败不得跳过验证，已有会话与比赛不依赖 RPC。

实现记录（2026-07-13）：`extraction-server` 使用独立锁文件固定 `signinwithethereum 0.8.1 + alloy` 与 SQLx/PostgreSQL 依赖；锁文件审查确认只有新增包、没有替换既有版本。上游 `alloy` 功能会开启元 crate 默认功能，进一步缩小依赖必须自行维护 EIP-6492 deployless validator 字节码与 ABI，当前保留上游安全关键路径而不复制实现。新增只增不删 migration，覆盖账号/钱包、一次性 nonce、单账号会话、比赛参与者、带幂等键与 digest/config version 的结算明细、仓库余额和资产流水。nonce 由 CSPRNG 生成并只存 SHA-256，5 分钟过期且在登录事务内原子消费；事务先取得 nonce、账号与当前活跃会话行锁，再读取权威数据库时间并同时复核 nonce 和服务端派生的 SIWE expiration/max-age 截止时间，任何锁等待跨过截止时间都会回滚且不替换旧会话。匿名 nonce/verify 入口在数据库或 RPC 前按直接 peer 做有界限流；过期 nonce 以有界批次、仓储硬上限和后台单飞任务定期清理，清理失败不阻断签发。SIWE 固定 Ethereum Mainnet，校验 domain、URI、scheme、nonce、chain、issued-at、expiration 和签名，验签及仓储返回后均使用新时钟再次检查截止时间。EOA 先本地 EIP-191 验签，EIP-1271 使用 Alloy 直接调用 `isValidSignature(bytes32,bytes)` 保留 RPC 故障分类，EIP-6492 进入 universal-validator 路径；两者均有 chain preflight、立即并发准入和 3 秒总超时，RPC 失败关闭且不参与已有会话、比赛或链下结算。

会话使用 32-byte 随机不透明 token，数据库只存 hash；HTTPS 强制 `__Host- + Secure + HttpOnly + SameSite=Lax + Path=/`，只有 loopback HTTP 开发地址允许非 Secure Cookie，公开 Origin 必须与 SIWE URI 同源，且公开 `run(config)` 会在任何启动动作前复核程序化配置，认证/仓库响应禁止缓存。HTTP/WS 共用同一认证服务，WebSocket principal 只来自 Cookie 对应的服务端 `account_id/session_id`，`Connect` 返回后会再次校验同一 Cookie，避免注册竞态。活跃连接保存 `min(absolute, idle)` 精确截止时间，每 30 秒只读复核数据库且不延长 idle；客户端活动在转发二进制请求前按需刷新会话，同一未变化临近截止时间每周期最多刷新一次，撤销或过期立即以 policy close 收敛。显式 logout 和同钱包二次登录都会撤销数据库 session，并通过 `CloseAuthenticatedSession` 幂等关闭 lost、pending Join、in-world、pending rebind 及 Leave 过渡 socket；World 移除期间仍保留 pending rebind 登记到回调收敛，避免注销窗口漏关连接。会话 touch 与 nonce 消费均先取行锁、再取数据库时间，系统时钟倒退或锁等待跨过截止时间都不会错误放行；账号行锁保证同钱包并发登录按顺序替换。新登录和 matchmaking 各有独立 feature flag，关闭时返回服务不可用且不恢复 guest/shared-secret。

验证记录：根 `voxelize` 42/42 lib 测试、应用默认 `--all-targets` 30/30、`engine` `--all-targets` 33/33、真实 PostgreSQL 隔离库 11/11、应用 `engine,db-tests` 全目标 Clippy `-D warnings`、应用与根定向 rustfmt、`cargo check -p voxelize --all-targets` 和 `git diff --check` 均通过。覆盖官方 EOA 向量、EIP-1271/EIP-6492、RPC HTTP/JSON-RPC 故障与无等待并发上限、错误 chain/domain/URI/signature、重复 nonce、后台单飞清理、同钱包并发登录、时间戳倒序、nonce/session 持锁等待跨 TTL、账号锁及旧会话锁等待跨 SIWE 截止时间、单调会话时间、环境与程序化 Cookie 降级拒绝、Origin/SIWE 同源校验、缓存/仓库/排队/注销、WS 注册后二次校验、持续只读复核、活动续期、临期写限频与精确过期关闭；migration 已在保留的本地隔离测试库成功执行。根引擎仍输出既有 84 条基线警告，本阶段应用代码无新增 Clippy 告警。官方客户端监听钱包地址、网络和断开事件并调用 logout 的接线保留在阶段 9，不在本阶段冒充已完成。

文件规模说明：认证服务已把 nonce 清理拆到 50 行专责模块，主体为 313 行，超出 300 行软上限的部分是最终 SIWE 截止时间派生及登录/会话/注销编排，仍低于 500 行硬上限；PostgreSQL nonce/login 与 session 分别拆为 283/141 行。`server/server/websocket.rs` 为 341 行，测试已移到独立 `websocket_tests.rs`，持续会话守卫另拆为 188 行 `ws_auth.rs`；剩余主体是握手、Actor 注册、I/O select、出站控制和关闭收敛的一组状态机，低于 500 行硬上限，后续业务逻辑不得继续堆入。既有 `server/server/mod.rs` 为 1053 行，本阶段只加入公开 Actor 消息、连接状态接线和短 Handler；同目录 `SIZE_NOTES.md` 已记录原因、约束和后续拆分计划，继续为单个消息机械拆分主 Actor 会扩大无关回归范围。

验收：API 测试完成 nonce -> SIWE -> session -> principal；错误链/domain/URI/签名、重复 nonce 均拒绝；RPC 故障不影响已有会话，官方客户端换地址/网络触发 logout 后旧 Cookie 被拒绝。

回滚：migration 只增不删；认证 feature flag 关闭时停止登录/匹配，不恢复伪造身份入口。

## 阶段 3：恰好 10 人比赛与 World 生命周期

- [x] 实现 FIFO queue、每账号一个非终态席位、9 人 Waiting、第 10 人原子冻结名单、第 11 人拒绝；终态后释放互斥以允许重新排队。
- [x] 实现 `Waiting -> Preparing -> Active -> ExtractionOpen -> Settling -> Finished|Aborted` 与参与者单向状态。
- [x] Preparing 断线时中止未开局批次、清理部分 World，并按原排队时间恢复其余在线玩家，不创建成绩或资产。
- [x] 创建独立内存 World，底层 320 范围、玩法精确 300 边界；写入 seed 和三类版本号。
- [x] 开局记录 `+8m`、`+12m` 绝对 deadline；全部终态可提前进入 Settling。
- [x] Waiting 断线离队；Active 断线 detach，60 秒内同账号 rebind，超时只进入一次 TimedOut 并按 generation 安全 despawn；死亡掉落接入保留到阶段 7。
- [x] Finished/Aborted 冻结生命周期写入并清理本阶段创建的 World、连接租约、pending tick/request 和协调器局内状态。
- [x] 把 20 半心实际安装为服务端权威玩家 ECS 状态，并用不同永久仓库存量验证入场状态完全一致；固定装备和 12 格背包已在阶段 5 完成，生命组件留在阶段 7。

实现记录（2026-07-13）：新增单进程有界命令协调器，统一排序 HTTP 排队、WS 连接生命周期与 fake-clock tick；只有已认证且存在游戏 socket、已绑定 World runtime、无非终态席位的账号可以进入 FIFO。第 10 人将恰好 10 个唯一账号原子固化为 `0..9` 座位，第 11 人失败；PostgreSQL 创建按 UUID 顺序锁账号，状态写按 match 后 participant 的固定顺序加锁，一致性读取使用可重复读。Preparing 仅在 10 个 World Join 全部提交后激活，任一名单成员断线会中止、停止部分 World，并按原时间与次序恢复其他在线玩家。

每局使用 `saving(false)` 的动态 World，底层 chunk 范围 `[-10,-10]..[9,9]`、玩法边界 `[-150,150)`；`MatchWorldSpec` 冻结 10 人、12 格背包、20 半心和相同初始装备，固定装备与背包已在阶段 5 安装为玩家 ECS 组件，生命组件仍留在阶段 7。比赛持久化 seed 与 generation/gameplay/config 三类版本，记录 `+8m`、`+12m` 和结算宽限绝对时间；数据库返回延迟不改变绝对时刻，独立 watchdog 会在协调器等待 SQL 时先关闭 gate 并停止 World。断线重连采用精确 60 秒边界，public player ID、generation 与唯一 attach attempt 共同阻止迟到 prepare/rebind/despawn 回调影响新租约或同名新 World。Settling/Aborted 先停 World 后写数据库，失败由后续 Tick 幂等重试；ticker 最多保留一个待处理 Tick。未启用引擎或尚未绑定 runtime 时排队从第 1 人开始即失败关闭。

持久化创建遇到未知结果时复用完全相同的 match ID/名单/seed/version，PostgreSQL 只接受完全一致的幂等重试。服务启动先用专用 PostgreSQL 连接取得进程级 advisory lock，第二实例在执行恢复前即失败；随后用带 lock/statement timeout 的事务原子中止遗留非终态比赛和参与者，保留终态参与者及已提交 settlement，重复恢复返回 0。该失败关闭恢复提前完成了阶段 8 的启动清理子项，但不等同于结算 reconciliation。当前专用锁连接尚无存活监测，部署时数据库/网络会话丢失必须停止并重启旧实例，禁止重叠替换进程。

验证记录：根引擎 lib 55/55；应用默认全目标 61/61，`engine,db-tests` 全目标 84/84，其中 `engine` lib 34/34、真实 PostgreSQL matchmaking 7/7、认证 11/11。应用全目标 Clippy `--no-deps -D warnings`、定向 rustfmt 与 `git diff --check` 均通过；根引擎仍输出既有 84 条基线 warning，本阶段应用代码无新增 Clippy 告警。覆盖 9/10/11、未知创建重试、Preparing 备用 socket 断线中止与 FIFO 保留、激活期间断线、绝对 deadline、Settling 首次写失败后的单次 World 停止、永久 pending World stop 的有限超时、60 秒重连准入与 timeout claim 两种线性化顺序、Join ABA、重复 Rebound、generation/attach attempt 隔离、进程锁和启动恢复。仍未完成真实 10/11 个网络 Actor 连续多局泄漏验收、玩家 ECS 生命，以及移动的体素碰撞 sweep/完整竖直运动权威；固定装备和背包已在阶段 5 补齐，现有移动解析器仍只提供有限数、状态、300 边界和速率过滤，这些缺口不能作为 PVP 生产公平性验收。

回滚：先支持单进程房间，不引入 Redis 或跨进程 World。

## 阶段 4：确定性地图与版本化资源

- [x] 注册稳定资源/装备定义并实现只依赖 seed/version/config 的 ChunkStage。
- [x] 泥土全图常见；黄金中层至少两个热点；钻石更稀少、深层、中心偏置且至少两个热点。
- [x] 配置密度、矿脉、深度、出生点、撤离候选和首版 `1/10/100` 统计权重；已被比赛引用的版本不可原地修改或追溯重估。
- [x] 固定 seed 测试 fingerprint、热点连通分量、数量/深度区间和不同 seed 差异。

验收：相同输入地图完全复现，新比赛不继承上一局地形。

回滚：调参只新增版本；旧版本保留复现能力。

实现记录（2026-07-13）：动态比赛 World 现在从完整 `u64 seed + generation-v1 + balance-v1` 构造不可变生成计划，使用全局坐标纯函数和仅写当前 Chunk 的 Stage；全局 Block Registry 与每局 Item Registry 均来自 manifest 显式 ID，资源物品携带 64 堆叠组件，装备不占资源堆叠。V1 固定 300×300 平坦地形、10 个公平出生点、5 个中层黄金连通热点、3 个更深且中心偏置的钻石热点，以及 8 个对称撤离候选；比赛 World 预加载出生核心并只在 `Ready` 后开放 generation，60 秒内未就绪会幂等回滚运行时 World 和路由。

验证记录：固定 seed 的实际 Chunk 输出 SHA-256 为 `234903c7af2917afb0e3b9aa643f5848c40f8e12b5494cd2b4d18187bb881df5`；正序/逆序 Chunk 处理一致，折叠 engine seed 相同的两个完整 64 位 seed 仍产生不同地图。扫描断言泥土 `4,387,170`、90,000 个泥土地表列、黄金 `20,265`（5×4,053）和钻石 `2,565`（3×855），并覆盖深度、中心偏置、Chunk 边界、无跨 Chunk 写、撤离候选覆盖及新局不继承旧 Chunk 修改。正式 PVP 仍需在后续安全加固中限制客户端任意 Chunk `LOAD`，当前不能宣称已具备 anti-Xray。

## 阶段 5：权威背包、掉落与自动拾取

- [x] 实现 12 格私有 `MatchInventory`、固定装备、64 堆叠和单调 revision。
- [x] 所有增减走领域方法，按已有堆叠/空格顺序返回 accepted/remainder。
- [x] 实现确定性 Drop ID、`PendingDropQueue`、世界 Loot、空间桶合并和比赛结束清理；掉落不按 TTL 删除。
- [x] 自动拾取在服务端按距离/seat 稳定排序，原子更新背包和掉落剩余。
- [x] 整组丢弃校验 sequence/slot/revision，丢弃者屏蔽 2 秒；不支持拆分或固定装备。

验收：64/65、满包、部分接纳、双人争抢、整组丢弃及 `背包 + pending + 地面` 守恒测试通过。

回滚：发现复制风险时先关闭新匹配，不用负余额或删流水补偿。

实现记录（2026-07-13）：动态比赛 World 在出生点 modifier 后组合安装稳定 seat/account/public ID、独立固定镐/近战装备与空 12 格资源背包；rebind 保留原实体和 revision。资源只以 `ResourceKey` 存入私有领域容器，批量拾取先在副本试装再一次提交；局内所有资源始终由背包、按 Drop ID 有序的 pending 队列或 `LootDropComp` 保管。手动丢弃从顶层协议 envelope 取得 u32 sequence，payload 只允许 slot/revision，先转 pending 再清整槽；确定性 ID 绑定 match/seat/sequence，本人精确屏蔽 2 秒。玩法系统在默认 dispatcher 全部叶节点后按手动丢弃、pending 生成/合并、自动拾取和 Direct 私有同步执行；掉落实体无 TTL，World 移除时整体清理。

验证记录：纯领域 16 项覆盖 64/65、满包/部分接纳、多资源一次 revision、距离/seat 双人争抢、满包候选跳过、整槽丢弃、2 秒边界、pending 冲突、保护合并和溢出不变；Engine 定向 4 项包含 2 项真实 ECS、1 项 authority 失败关闭和 1 项 envelope 错误分类，覆盖 pending -> 世界 Loot -> 自动拾取、整槽保护、重复 sequence 幂等及错误版本/坏结构回包。Rust 共享契约 6 项、TypeScript 共享契约/客户端 18 项、应用默认全目标与 Engine 全目标、根引擎 57 项、客户端生产 build、应用 `--no-deps -D warnings` Clippy 和 `git diff --check` 均通过；根引擎仍只有既有 84 条 warning 基线。所有新增生产文件低于 300 行，测试文件适用规模例外。

## 阶段 6：服务端权威挖掘

- [x] 客户端只发送 start/maintain/cancel 和 sequence，服务端校验状态、阶段、工具、边界、距离、视线和方块。
- [x] fake clock 驱动泥土/黄金/钻石 `0.5/1.5/3s`；所有取消条件从零重置。
- [x] 完成时先用 `HarvestedVoxelSet` claim，再写 AIR、固定产出 1、入包或 pending drop。
- [x] `MiningResolution -> ChunkUpdating` 顺序固定；strict World 始终禁用客户端经济方块 raw UPDATE。
- [x] 客户端进度只显示服务端 revision 状态。

验收：同体素重复/乱序/并发完成最多产出 1；满包仍破坏一次并留下 1 个公共资源。

回滚：不可为兼容重新开放 raw UPDATE。

实现记录（2026-07-13）：新增严格的 `pvp:v1:mining` start/maintain/cancel 协议、独立 Direct `pvp:v1:mining-state` 完整快照和客户端单调 sequence 工厂；客户端不能提交资源、体素 ID、时长、进度、完成或数量。服务端以固定基础镐、300×300/Y 边界、Ready Chunk、4.5 格距离及首个非 Air 体素射线为权威验证，并在每次意图和 tick 持续复核。`balance-v1` 冻结 `500/1500/3000ms`、350ms maintain grace 和 50ms 同步粒度；换目标、松键超时、失去权限/工具/距离/视线、方块变化或竞争失败均从零重置。

完成事务先在 `MiningState` 克隆上准备不可失败的完成态，再由 World 级 `HarvestedVoxelSet` 原子 claim；固定数量 1 进入背包，满包或库存 revision 耗尽则进入确定性公共 pending drop，成功后 staging AIR 并安装完成态。默认 dispatcher 增加只能安装一次且固定依赖 `CurrentChunk` 的命名 hook，保证 `MiningResolution -> ChunkUpdating` 同 tick 消费 AIR；自定义 dispatcher、系统名冲突和重复安装显式失败，strict World 的单条/批量 raw UPDATE 继续在 staging 前拒绝。服务器同 tick 可合并多个 revision，客户端把每条消息视为完整快照，接受任意更高 revision 且本地计时不能推进权威进度。

验证记录：纯领域 25 项覆盖三资源精确毫秒边界、grace 前/等于/超过、sequence/换目标/cancel、亚毫秒配置失败关闭、完成态预提交、claim/回滚、满包/revision 耗尽和资产守恒；Engine gameplay 8 项覆盖真实 Ready Chunk 精确完成、两人同体素稳定竞争、满 12 格 pending、遮挡/未 Ready/零与 NaN 方向，以及同一 dispatcher 内 staging AIR 后由 `ChunkUpdating` 落地。Rust/TypeScript 共享契约、客户端 37 项、根引擎 61 项、应用 Engine 全目标、客户端生产 build、应用 `--no-deps -D warnings` Clippy 和 `git diff --check` 均通过；根引擎仍只有既有 84 条 warning 基线。所有新增生产文件低于 300 行，测试文件适用规模例外。

## 阶段 7：近战、生命、死亡与结果

- [x] 实现 20 半心、每次伤害 2、0.6 秒冷却、3 格距离、完整方块遮挡和无回血。
- [x] 攻击只提交 sequence/武器槽；服务端 ray-AABB 选择最近合法目标，落空也消费冷却。
- [x] 固定同 tick 优先级，死亡早于拾取和撤离；`Alive -> Dead` CAS 只执行一次。
- [x] 死亡冻结背包并转入唯一 pending loot，固定装备不掉，记录击杀者和结果统计。
- [x] 断线角色可被攻击；窗口内被杀和超时不能双掉落。死亡结果后离开原 World，不接收观战数据。

实现记录（2026-07-13）：新增严格 `pvp:v1:attack` 协议，payload 只允许固定近战槽，目标、伤害、生命、击杀与队伍元数据均由服务端拒绝。每局玩家实体安装 20 半心、固定近战装备、攻击 sequence/cooldown revision、局内统计和不可逆淘汰组件；loadout 最大生命与协议 20 不一致时在创建 World 前失败关闭。服务端用权威 `PositionComp/DirectionComp` 对所有其他存活玩家执行 `0.8 x 1.8` ray-AABB，按距离和 seat 选择最近目标，Ready Chunk 内完整单位方块遮挡，未知或未 Ready 数据失败关闭；落空同样消费 600ms 冷却，同玩家网络接收顺序不按 sequence 重排。

Combat 命名 hook 固定在 Broadcast 前，重连超时意图先于攻击解析，死亡在同 tick 早于 pending 生成和自动拾取。死亡事务先在副本准备生命、挖掘重置、存活/资源/击杀统计和结果，再原子冻结并排空 12 格背包，把全部资源转为 `drop:v1:{match}:seat:{seat}:death` 唯一 pending 所有权；固定装备不进入掉落。`EliminationComp` 保证 World 内只产生一个终态，PostgreSQL `mark_dead/mark_timed_out` 使用 match-first 行锁和精确 killer/stats 幂等 CAS，旧无统计 `time_out` 接口已移除。终态 Direct 在 Broadcast 发出后，matchmaking 先关闭 gate，并在等待 SQL 前执行 generation-safe 有限驱逐；卡住或失败进入 Tick 重试。终态玩家可以在旧局清理前进入下一等待队列，但满 10 人不会并行创建第二个 World；fail-closed 后 ticker 也不会创建不可 Join 的比赛。

验证记录：应用 `engine` lib 现有 112 项全部通过，其中 ECS combat 覆盖真实 10 次攻击、第 10 次唯一死亡、599/600ms、落空冷却、精确 3 格/略超距离、最近目标、完整方块遮挡、断线目标、乱序 sequence、超时与攻击同 tick、唯一掉落和击杀统计；matchmaking 覆盖在线/断线死亡、超时统计、旧 generation、rebind 拒绝、驱逐错误/永久挂起超时、落库失败关闭和终态重排队。共享 Rust 契约 9 项、客户端 79 项、根引擎 64 项、E2E Actor 2 项、客户端生产 build、全 extraction ESLint、应用默认与 `engine,db-tests` Clippy `--no-deps -D warnings`、JSON 与 `git diff --check` 均通过。真实 PostgreSQL 新 CAS 测试已编译但本次环境无 `TEST_DATABASE_URL/DATABASE_URL`，因此不得声称执行；根引擎仍只有既有 84 条 warning 基线。

剩余跨阶段约束：断线死亡/超时玩家无法接收原 World Direct，完整结果的耐久恢复和硬截止前终态通知封口并入阶段 8；当前位置入口仍只有有限数、300 边界和速率预算，完整体素碰撞 sweep、重力/落地/跳跃权威必须在发布门禁前完成。上述两项完成前不能把当前版本称为公平 PVP 发布候选，因此本阶段最后一项保持未完成。

验收：第 10 次合法命中死亡一次；冷却、距离、遮挡、伪造目标/伤害、旧 sequence 和重连攻击均拒绝；team/role 元数据不产生友军豁免，重连不回血而新比赛恢复 20 半心。

回滚：战斗入口 feature flag 可关闭，但资源守恒与核心 hardening 保留。

## 阶段 8：撤离、结算与异常恢复

- [x] 第 8 分钟才发布唯一撤离区；服务端累计连续 8 秒，离区/死亡/断线中断；仅 `qualified_at <= hard_deadline` 进入待结算。
- [x] 达标后冻结 participant 和背包，生成规范快照、digest、config version 与 `extract:v1:{match}:{account}` 幂等键。
- [x] 异步事务原子写 settlement/items、ledger、warehouse 和 participant Extracted；World tick 不等待 SQL。
- [x] commit 后才确认；未知结果先查询，commit 前失败为 0，commit 后响应丢失仍为 1。
- [x] 实现硬截止后 30 秒写入宽限期；期满不再发起写入，仅保留有界只读 reconciliation，并允许清理玩法 World。
- [x] 启动时把未结束比赛标 Aborted，保留已提交收益，其他局内收益作废；完整 settlement reconciliation 仍留在本阶段后续子项。
- [x] 实现仅允许本人访问的 `/api/matches/{match_id}/result` 与 `/api/matches/latest-result`，覆盖 PendingReconciliation/Extracted/Aborted。
- [x] 持久化完整死亡/超时结果，使断线时未收到 Direct 的本人可在大厅恢复击杀者、存活时间和资源统计。
- [x] hard deadline 关闭 gate 后先封口 World 已发生的死亡/超时 outbox，再进入 participant 批量终态；携带权威发生时间，不能把截止前死亡覆盖为无统计 TimedOut。
- [x] 仓库只读展示数量和 settlement 聚合统计，不提供消费/交易/属性入口。

实现记录（2026-07-13）：撤离区在持久化 `ExtractionOpen` 后才通过 Direct 状态公开，服务端以单调时钟计算连续 8 秒，离区、死亡和断线均清零；硬截止采用包含边界并固定执行“死亡/攻击 -> 撤离达标 -> 剩余玩家 hardDeadline 终态 -> Broadcast -> death/extraction outbox -> seal -> stop”。达标时冻结 12 格背包，按固定键序聚合三类资源并生成 SHA-256 digest 与稳定幂等键；所有后续玩法入口拒绝 `SettlementPending`。独立异步协调器在 World tick 外执行 mark/commit/find，未知 commit 先读后写，宽限期后最多有界只读核对且不再写库。审查补充修复了 `seal_hard_deadline=Ok(false)` 被误当成功的问题，此时比赛失败关闭为 Aborted。

PostgreSQL 结算使用数据库事务时间、5 秒锁超时、15 秒语句超时和固定 `match -> participant -> account -> settlement/assets` 锁序，在一个事务中写 settlement/items、append-only ledger、warehouse 与 Extracted；同 digest 重试不要求重建相同 settlement UUID，空背包与累计溢出均有回归。本人结果 API 使用会话账号查询，跨账号统一返回 `200 null`，覆盖待核对、撤离、死亡、超时和异常五种状态；死亡/超时原因、时间、存活时长及资源统计持久化，仓库数量和聚合统计在同一只读可重复读快照中返回。新增生产文件均低于 300 行；大型测试文件适用测试例外。

验证记录：应用 `engine` 全目标实际运行 172/172，通过后新增 `Ok(false)` 回归并定向通过；`engine,db-tests` 全目标 check 与 `--no-deps -D warnings` Clippy 通过，客户端 85/85、生产 build、全 extraction ESLint、E2E Actor 2/2、根引擎 64 项及根集成测试均通过，`git diff --check` 通过。PostgreSQL 用例已编译，但当前未配置 `TEST_DATABASE_URL/DATABASE_URL`，没有连接数据库或执行 migration，因此不声称真实 SQL 测试通过。

验收：fake clock 覆盖 8m/8s/12m/30s 边界；截止前达标且截止后 commit 仍成功，截止后达标失败；故障注入覆盖 commit 前、回滚、commit 后响应前、宽限期后只读核对和重启核对。

回滚：不删除/反向修改 settlement 与 ledger；暂停新匹配并只读 reconciliation。

## 阶段 9：产品客户端与 Reown/UI

- [ ] 实现实际登录/大厅/比赛/结果/仓库首屏流程，不制作营销落地页。
- [x] 集成 AppKit Ethers adapter、Mainnet 和 SIWE；连接钱包但未认证不能查仓库或排队。
- [x] HUD 显示稳定 10 心/半心、12 格背包、固定装备、阶段倒计时、挖掘和撤离进度。
- [x] 死亡结果显示击杀者/存活/资源得失；撤离仅在 commit 后显示永久入账；重新登录可恢复待核对或异常比赛结果。
- [x] 重连恢复全量 revision；换钱包/网络调用服务端 logout、关闭 WS 并清理控制状态。
- [x] 单一 typed decoder/reducer 拥有状态；测试 bridge 只进入测试构建。

实现记录（2026-07-13）：产品客户端已落地钱包登录、大厅匹配、HUD、结果和仓库首屏；AppKit 仅启用 Ethereum Mainnet + SIWE 身份链路，关闭交易、Swap、Onramp、Send/Receive、Email 和 Social 功能。服务端会话与当前钱包地址、链严格绑定，慢签名不被 15 秒误取消；换钱包、换链和退出均先使当前认证 generation 失效，再调用服务端 logout 并关闭 WebSocket。登录、排队、比赛恢复和结果轮询都使用 generation 防止旧响应污染新会话或新比赛。

实时状态由严格 decoder、单一 reducer 和 revision 单调规则拥有；重连只使用服务端自动 rebind，不重复 JOIN，并在 60 秒内恢复完整玩法快照。攻击、挖掘和丢弃共享服务端确认的全局 sequence cursor；背包快照增加必填 nullable `lastDropSequence`，刷新后从三类权威游标最大值继续。终态结果单调，旧比赛迟到结果不能中断新比赛；在线/断线非终态不会被误显示为 Aborted。

响应式验收覆盖 1440x900、390x844、320x844、568x320 和 667x375：10 颗心、12 格背包与固定装备不越界，短横屏准星不覆盖生命条，登录/大厅/结果面板末端操作可滚动触达；canvas 非空且连续帧发生变化。`state.ts` 为 310 行，仅超过 300 行软上限且低于 500 行硬上限；它集中定义同一 reducer 的完整判别联合与转换，当前拆分会分散状态所有权，因此暂不拆分。

当前明确阻断：生产客户端仍未消费 Voxelize 压缩 INIT/UPDATE/LOAD，也没有把键鼠、指针锁、移动、瞄准、挖掘、攻击和丢弃接到真实 300x300 服务端 World。现有 Three 场景只用于 UI 与视觉验证，不能称为实际可玩的对局；第一项必须在阶段 11 的真实世界接入和权威移动闭环完成后才能勾选。

验证记录：客户端 Vitest 22 个文件、154/154 通过；TypeScript、全 extraction ESLint、生产 build 与测试后门扫描通过。Rust `cargo fmt --all -- --check` 与应用 `engine` Clippy `--no-deps -D warnings` 通过；根 Voxelize 仍只有既有 84 条 warning。浏览器逐视口检查无页面横向溢出或不可达操作。真实 PostgreSQL 未配置且未执行 migration。

验收：Vitest 覆盖钱包/换链注销、revision、心形 HUD、待核对/异常结果；模拟 EIP-1193 provider 断言闭环中从不调用交易方法；生产 build 不含测试后门。

回滚：UI 与版本化契约可独立回滚，环境配置不写死。

## 阶段 10：内部只读运维

- [ ] 提供账号、比赛、participant、settlement、warehouse 和 ledger 的固定只读查询。
- [ ] 使用独立只读角色、内网/CLI 边界、分页、限流、超时和脱敏访问日志。
- [ ] 拒绝匿名、普通玩家、写请求、任意 SQL、余额修改和 settlement 重放。

验收：授权查询成功，所有写入尝试失败且访问有不泄密的审计记录。

回滚：运维入口可整体关闭，不影响游戏。

## 阶段 11：E2E、观测与发布门禁

- [ ] 在公平 PVP 验收前实现服务端体素碰撞 sweep、重力、落地和跳跃权威；合法速率内的小步穿墙/飞行也必须拒绝。
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
