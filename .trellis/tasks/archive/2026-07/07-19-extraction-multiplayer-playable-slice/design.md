# 多人联调可玩切片 — 技术设计

## 目标边界

- **改**：匹配人数可配置（DEV）、本地文档、联调验证与必要测试。
- **不大改**：战斗/挖掘/撤离/结算规则本体、客户端生产登录流。
- **禁**：mock 登录、生产默认改 2 人、单机逻辑并入网络路径。

## 架构

```
Browser A (SIWE wallet1) ─┐
                          ├─► Vite :5173 ──HTTP/WS──► extraction-server :4100 ──► Postgres
Browser B (SIWE wallet2) ─┘         │                        │
                                    │                        ├ matchmaking (queue size = match_size)
                                    │                        └ engine world (Voxelize)
```

## DEV match_size

### 配置

| 变量 | 含义 |
|------|------|
| `EXTRACTION_DEV_MATCH_MODE` | `1`/`true` 时启用 DEV 成局（**且**仅 `debug_assertions` 或显式 non-production 构建时允许解析成功；release 生产二进制若设置则启动失败或忽略并 log） |
| `EXTRACTION_DEV_MATCH_SIZE` | `2..=10`，默认 `2`；仅 `DEV_MATCH_MODE` 开启时读取 |

**有效 match_size**：

```text
if dev_match_mode_allowed && env DEV_MATCH_MODE:
  clamp(DEV_MATCH_SIZE, 2, 10)  # default 2
else:
  10
```

推荐：**release 生产包**忽略 DEV 变量并固定 10；`cargo run`/本地 debug 可读 DEV。

### 代码影响（匹配层）

当前 `MATCH_SIZE` 与 `FrozenRoster([_; 10])` 把 10 焊死。

**方案（推荐）**：引入运行时 `MatchCapacity`（`usize`，2–10），替换固定数组为：

- `FrozenRoster { seats: Vec<FrozenParticipant> }` 长度 = capacity
- `SeatId` 校验上限 = capacity
- `QueueCoordinator` 持有 `capacity: usize`，`enqueue`/`prepare_first_roster` 与 `take(capacity)` 使用该值
- `MatchWorldSpec` / DB 写入仍存实际参与者列表；**schema 若假设 10 席**需确认是否允许更短名单

**兼容**：

- 生产 capacity=10 → 行为与现网一致。
- 既有测试：默认 capacity=10 的 fixture 不动；新增 capacity=2 的 unit 测试。

**风险点**：

- DB / 结算是否假设 10 行 participant — 需读 migrations；若只存实际 participants 则 N 人自然兼容。
- 引擎 world `max_clients` 是否 10 — 需 ≥ capacity。

### 不采用

- 假人补位：本切片不做 bot AI。
- 仅改队列阈值但 roster 仍填 10：会卡在缺账号。

## 联调文档

新增例如 `apps/extraction-server/docs/local-multiplayer.md` 或仓库 `docs/extraction-local-multiplayer.md`：

1. Postgres 创建库 + migrate  
2. `.env` 模板（含 `DEV_MATCH_MODE=1`、`DEV_MATCH_SIZE=2`）  
3. `cargo run -p extraction-server --features engine`（以实际 package 名为准）  
4. `pnpm --filter @voxelize/extraction-client dev`  
5. 两个 Chrome Profile + 两个钱包 SIWE  
6. 匹配 → 进世界 → 验收清单（互见/挖/砍/撤或死）  
7. 故障：进程锁、cookie domain、CORS、WS 路径  

## 客户端

- **无 mock 登录**；确保本地 `VITE_*` / cookie 与 SIWE domain 对齐。
- 若存在硬编码「10 人进度文案」，改为显示服务端返回的队列人数 / 目标人数（若 API 已有字段则接；没有则 DEV 文档写死「需 2 人」）。
- 不改 `?mode=single`。

## 验证策略

| 层级 | 内容 |
|------|------|
| Unit | capacity=2 成局、capacity=10 回归、非法 env |
| 手工 | 双浏览器 AC3–AC5 |
| 门禁 | typecheck、相关 cargo test、生产 build 无 DEV 字符串泄漏（可选 assert） |

## 回滚

- 关闭 `DEV_MATCH_MODE` 即回 10 人。
- 文档与 env 示例可独立回滚。

## 文件热点

- `apps/extraction-server/src/matchmaking/model.rs` — roster 结构  
- `coordinator_queue.rs` / `coordinator_timing.rs` — 成局阈值  
- `match_world.rs` — capacity 常量引用  
- `config.rs` — env 解析  
- 客户端 match UI / coordinator（队列展示）  
- 文档 + `.env.example`  
