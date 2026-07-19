# Extraction 多人联调可玩切片

## Goal

在已归档的 PVP MVP 之上，交付一条**可重复的本地多人联调路径**：起 Postgres + extraction-server → 双（或 N）浏览器真实 SIWE 登录 → DEV 可配置人数成局 → 同图互见 → 完成权威挖矿、近战，以及撤离或死亡之一 → 有明确结果；且**不破坏生产 10 人匹配与生产构建边界**。

## Background

### Confirmed Facts

- 产品：桌面浏览器、钱包身份、固定 **10 人**、权威采集/战斗/撤离/仓库（`PRODUCT.md`）。
- PVP MVP 任务已 **completed** 并归档（`archive/2026-07/07-12-voxel-extraction-pvp-mvp`）。
- 成局容量：`MATCH_PLAYER_CAPACITY = 10` / `MATCH_SIZE = 10`；`FrozenRoster` 为固定长度数组（`match_world.rs`，`matchmaking/model.rs`）。
- 队列在 `queue.len() == MATCH_SIZE` 时 `prepare_first_roster`（`coordinator_queue.rs`）。
- 本地服默认 `127.0.0.1:4100`，SIWE/origin 默认对齐 Vite `127.0.0.1:5173`；依赖 `DATABASE_URL` 与 matchmaking 进程锁。
- 客户端无 DEV mock 登录；生产入口为 Reown SIWE；另有 `?mode=single` 与 e2e 模式。
- 当前分支 `codex/extraction-single-hand-sword` 包含 PVP stage1 与单机切片。

### Problem Restatement

MVP 能力在，但本地必须凑满 10 账号才能成局，联调成本过高，阻碍多人体验迭代。

### Fundamental Constraints

- 生产路径保持 10 人 + SIWE + 服务端权威。
- DEV 捷径不得泄漏生产 bundle 或默认匹配。
- 身份本切片使用**真实 SIWE**（不引入 mock 登录）。

## Requirements

### R1. 本地起服与文档

- 可执行联调文档：Postgres、env、起服、客户端、双钱包/双配置文件、常见失败。
- health/API 可达；单 matchmaking 进程。

### R2. DEV 可配置成局人数

- 环境变量 `EXTRACTION_DEV_MATCH_SIZE`（默认 **2**，合法范围 **2–10**）。
- 仅当进程显式处于 DEV 联调模式时生效（见设计：非 release 或 `EXTRACTION_DEV_MATCH_MODE=1`）。
- 生产 / 未开 DEV 模式：**忽略该变量，恒为 10**。
- 队列阈值与 roster 长度均使用有效 `match_size`，测试覆盖 2 与 10。

### R3. 同局互操作（验收级）

- ≥2 客户端同 world **互见**（对方角色位置可见）。
- 至少一次**服务端确认的挖矿**完成并入包或掉落。
- 至少一次**服务端确认的近战**伤害/击退（或击杀）。
- 至少完成 **撤离成功** 或 **死亡掉落** 其一，并出现可理解结果 UI。

### R4. 边界

- `?mode=single` 不变。
- 生产 build 无 DEV 成局入口；默认配置 10 人。

## Acceptance Criteria

- [ ] AC1：按文档可本地起服并打开客户端。
- [ ] AC2：DEV 模式默认 2 人成局；`EXTRACTION_DEV_MATCH_SIZE` 可调 2–10；非 DEV 恒 10。
- [ ] AC3：双客户端 SIWE 后同局互见。
- [ ] AC4：双客户端演示挖矿（权威）与近战（权威）各至少一次。
- [ ] AC5：完成撤离或死亡结果展示至少一种。
- [ ] AC6：生产相关测试/typecheck 通过；生产 build 无 DEV 泄漏。

## Out of Scope

- Mock/测试账号登录、观战、组队、新武器、地图大改、链上资产。
- 完整 10 人真人压测、商业大厅重做。
- 单机进度同步仓库。

## Decisions

| 项 | 决定 |
|----|------|
| 切片类型 | 可玩联调，非从零重做 MVP |
| DEV 人数 | 可配置 N，默认 2 |
| 身份 | 真实 SIWE + 多钱包 |
| 玩法验收 | 互见 + 挖矿 + 近战 + 撤离/死亡其一 |
| 分支 | 建议 `codex/extraction-multiplayer-playable-slice` from 当前 HEAD |

## Notes

- 复杂任务：需 `design.md` + `implement.md`，用户评审后 `task.py start`。
