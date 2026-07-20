# 多人联调可玩切片 — 实施计划

## 实施边界

- 优先改 matchmaking 容量与配置；玩法规则仅在联调阻塞时修 bug。
- 不引入 mock 登录；不改单机 `?mode=single`。
- 分支建议：`git checkout -b codex/extraction-multiplayer-playable-slice`（from 当前 HEAD）。

## 阶段 0：分支与基线

- [x] 创建联调分支 `codex/extraction-multiplayer-playable-slice`；server 可编译。
- [x] 文档草稿：`docs/extraction-local-multiplayer.md`。

## 阶段 1：DEV match_size 配置

- [x] `ServerConfig` 解析 `EXTRACTION_DEV_MATCH_MODE` / `EXTRACTION_DEV_MATCH_SIZE`。
- [x] 未开 DEV mode → 恒 10；开启后默认 2，可调 2–10。
- [x] capacity 注入 `MatchmakingService` / `Coordinator`。

## 阶段 2：Roster / 队列去固定 10 数组

- [x] `FrozenRoster` 改为 `Vec`，长度 2..=MATCH_SIZE。
- [x] 队列阈值使用 `self.match_size`。
- [x] Postgres create/activate 使用实际 roster 长度。
- [x] 测试：`dev_match_size_two_forms_roster_without_ten_players`、`match_domain`。

## 阶段 3：客户端队列展示（若需要）

- [x] 客户端无硬编码「10/10」文案；DEV 文档写明 N=2。
- [x] 双客户端 cookie/WS 同 origin：`127.0.0.1:5173` ↔ `:4100`（文档 + browser smoke 验证）。

## 阶段 4：联调文档与 env 示例

- [x] 更新 `apps/extraction-server/.env.example`。
- [x] `docs/extraction-local-multiplayer.md`。
- [x] 本机验收（2026-07-20）：协议 smoke、dev-mp gameplay actor、双浏览器进局均通过。
  - 注意：上一局未释放座位时 browser 会 `MATCH_FULL`/卡大厅；约 1 分钟 reconnect timeout 或等 finished 后重试。
  - AC5 撤离成功路径本切片未自动化；**死亡掉落**已覆盖。

## 阶段 5：质量门

- [x] `cargo test --lib ... dev_match_size_two` 通过。
- [x] 协议层：`node apps/extraction-e2e/scripts/dev-two-player-smoke.mjs` PASS。
- [x] 玩法层：`vitest ... dev-mp-gameplay.actor.ts` PASS（挖矿/互见/击杀/死亡掉落拾取）。
- [x] 浏览器：`dev-two-browser-smoke.mjs` PASS（seat0/1 同 MATCH 已连接）。
- [x] `pnpm --filter @voxelize/extraction-client typecheck` 通过。
- [x] production build + `assert-production-boundary.mjs` 通过；`import.meta.env.DEV` 门控 `dev-mp` 不进生产包。

## 开始实现前门禁

- [x] 用户批准规划并 `task.py start`。
- [x] 切分支并实现阶段 1–2 + 文档。
