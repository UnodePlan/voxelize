# 玩法运行时与资产守恒研究

## 状态所有权

比赛顶层状态由单一 reducer/MatchSystem 管理：

```text
Waiting -> Preparing -> Active -> ExtractionOpen -> Settling -> Finished
                       \-------------------------------> Aborted
```

参赛者状态独立：

```text
Active | Disconnected | SettlementPending | Dead | Extracted | TimedOut
```

所有终态单向且幂等。比赛使用可注入单调 `Clock` 和绝对 deadline；生产保存对应 UTC 时间用于显示和审计，不能用累计 tick，因为 `Stats.delta` 会截断且 tick 可能延迟。

精确可玩区域为 `x,z in [-150,150)`。底层 chunk 为 16 时使用 `[-10,-10]..[9,9]` 的 `320 x 320` 引擎范围，MatchSystem 负责 300 边界、出生点、挖掘、移动和撤离校验。

## 组件与资源

组件：

- `MatchPlayerComp { account_id, seat_id, state, connected }`
- `HealthComp { half_hearts: u8, revision: u64 }`，范围 `0..=20`
- `CombatComp { last_attack_at, last_sequence, last_attacker }`
- `MiningComp { target, expected_block, started_at, last_maintain_at, sequence, revision }`
- `ResourceInventoryComp { slots[12], revision, last_drop_sequence }`
- `RoundStatsComp { mined, picked_up, lost, survival_started_at }`
- `LootDropComp { drop_id, contents, pickup_exclusion, revision }`

资源：

- `GameplayConfig` 与版本号；
- `MatchClock`；
- 有界 `AttackIntentQueue`、`MiningIntentMap`、`DropSlotIntentQueue`；
- `HarvestedVoxelSet`；
- `PendingDropQueue`、`SpawnedDropIds` 和掉落空间桶；
- 异步 `SettlementCommand/SettlementResult` 有界通道。

背包组件字段保持私有，所有变更只能经过 insert/remove/drop/freeze 等领域方法。

## 版本化消息

使用 JSON Method/Event，不让客户端上传权威结果：

```text
pvp:v1:attack      { sequence, weaponSlot }
pvp:v1:mining      { sequence, action, voxel? }
pvp:v1:drop-slot   { sequence, slot, expectedInventoryRevision }
pvp:v1:get-state   {}
```

服务端直发自身生命、背包、挖掘、阶段、死亡和撤离状态；附近玩家只接收 swing/hit/drop 等表现事件。每个私有快照带 revision，客户端忽略旧 revision，发现跳号或重连时请求全量快照。

## 挖掘原子性

```text
start/maintain intent
-> 校验 Alive/阶段/工具/边界/距离/视线/方块
-> MatchClock 达到 0.5/1.5/3 秒
-> HarvestedVoxelSet 原子 claim 坐标
-> 再核对方块类型并暂存 AIR 更新
-> 固定产生 1 个资源
-> 背包 insert，remainder 转 PendingDropQueue
-> 清理 MiningComp 并同步结果
```

`Chunks::update_voxel` 只写 `updates_staging`，不会立即改变可读方块（`server/world/voxels/chunks.rs:505-524`）。两个同 tick 完成者可能都看到旧矿块，所以必须先 claim 坐标；不能只依赖 chunk update 去重。`MiningResolutionSystem` 排在 `ChunkUpdatingSystem` 前。

客户端不得调用通用 `updateVoxels` 完成经济挖掘。松开、换目标、超距、失去视线、死亡、心跳超时或目标先被 claim 都从零取消。

## 战斗与死亡

```text
attack intent
-> 序号/Alive/阶段/固定武器校验
-> 消费 0.6 秒冷却（命中或落空都消费）
-> 服务端眼睛射线遍历最多 9 个存活 AABB
-> 选择 3 格内最近目标并做完整方块遮挡
-> Health 减 2 个半心单位，revision + 1
-> 归零时以 Alive -> Dead CAS 进入唯一死亡流程
```

主动攻击不能用 `CollisionsComp` 代替；碰撞只表示接触。MVP 不做护甲、暴击、远程、回血、延迟补偿或客户端目标选择。

死亡流程先冻结玩家和背包，再把所有资源转移到确定性 death drop 的 `PendingDropQueue`，随后清空背包并记录击杀与结果。队列也是合法资产保管方，所以实体生成延迟不会让资源凭空消失。重复伤害、断线超时或旧消息看到非 Alive 后直接 no-op。

同 tick 优先级至少保证：伤害/死亡早于自动拾取和撤离完成；死亡玩家不能在同 tick 捡走物品或成功撤离。

## 背包、掉落与自动拾取

背包固定 12 格、同类每格 64。insert 先填同类堆叠，再用稳定槽位顺序占空格，返回 `accepted/remainder`。固定装备不在该容器中。

资产在正常运行时始终属于且只属于：

```text
玩家背包 | PendingDropQueue | 世界 LootDropComp | 已提交永久仓库
```

- 自动拾取每 tick 使用服务端距离，不依赖只出现一次的碰撞 Started；
- 同一掉落候选按距离、再按稳定 seat ID 排序；
- 在一个临界区同时增加背包、减少掉落，剩余为零才删除实体；
- 整组丢弃用 `sequence + expectedRevision + slot`，先把整格 replace 为 Empty 并转入确定性 pending drop，再增加 revision；
- 手动掉落只排除丢弃者 2 秒，其他人立即可捡；
- 死亡背包可生成一个包含多资源的 loot bag；
- 为避免满背包持续挖泥土产生海量实体，DropSpawn 按邻近空间桶合并无拾取保护的兼容掉落，只合并保管方，不改变总量或过期删除；
- 所有地面资源保留到被捡或比赛 World 清理。

守恒测试必须把 pending 计入总量：`背包 + pending + 世界掉落` 在每次局内操作前后相等。

## 撤离与异步结算

第 8 分钟从版本化候选点按 seed 选择并公开唯一撤离区。服务端按权威位置累计连续 8 秒，离区、死亡或断线中断；第 12 分钟拒绝未完成玩家。

达到 8 秒后参与者先进入 `SettlementPending`，冻结背包并从攻击/拾取目标集合移除，再把不可变结算 payload 发给异步 worker。数据库 commit 成功后才进入 `Extracted` 并确认；失败使用同一幂等 ID 重试。World tick 绝不等待数据库 I/O。

全部参与者终态且 pending settlement 已返回后可提前结束比赛。`Finished/Aborted` 拒绝全部玩法写入，然后停止并移除 World。

## ECS 系统顺序

`set_dispatcher` 会整体替换默认链，因此必须复制全部核心系统并显式插入：

```text
UpdateStats -> CurrentChunk
-> MiningResolution -> ChunkUpdating -> ChunkRequests/Generating/Sending/Saving
-> Physics -> MovementValidation
-> CombatResolution -> DeathResolution -> ManualDropResolution
-> DropSpawn -> AutoPickup
-> ExtractionResolution -> MatchTransition
-> GameplayPrivateSync / GameplayPublicMeta
-> EntitiesSending / PeersSending -> Broadcast -> Cleanup -> Events
```

升级 Voxelize 时必须对比默认 dispatcher，防止漏掉核心系统。私有背包、生命和结算状态使用 Direct 消息，不进入会广播并 reset 的 Peer metadata。

## 地图与运行时清理

- 地图只由 `(seed, generation_version, config_version)` 决定；
- 泥土常见，黄金中层多热点，钻石深层、中心偏置且至少两个热点；
- 稳定物品键绑定显式 registry ID；
- Match World `saving(false)`，上一局地形、掉落和玩家状态不复用；
- Finished/Aborted 后停止 tick、清除连接映射、Actor、pending intent/drop/settlement channel 和渲染实体；
- 未提交局内资产随异常终止作废，数据库已提交 settlement 保留。

## 关键测试

- fake clock：0.5/1.5/3 秒、0.6 秒、8 分钟、8 秒、12 分钟、60 秒；
- 同坐标双挖只产出一次；
- 10 次命中只死亡一次；
- 双人并发拾取、部分容量和整组丢弃守恒；
- 死亡、拾取、撤离同 tick 的固定优先级；
- 9/10/11 人、迟到加入、断线 rebind、超时死亡；
- 连续多局无 World、连接、pending tick 或掉落实体泄漏。
