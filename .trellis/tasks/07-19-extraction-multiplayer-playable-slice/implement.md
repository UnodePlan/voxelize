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

- [ ] 若 UI 写死「10/10」，改为服务端目标人数或 DEV 文档说明（文档已写 N=2）。
- [ ] 确认双客户端 cookie/WS 同 origin 配置（见文档）。

## 阶段 4：联调文档与 env 示例

- [x] 更新 `apps/extraction-server/.env.example`。
- [x] `docs/extraction-local-multiplayer.md`。
- [ ] 本机双浏览器走通 AC3–AC5（需 Postgres + 双钱包，待人工）。

## 阶段 5：质量门

- [x] `cargo test --lib ... dev_match_size_two` / `match_domain` 通过。
- [ ] 更全量 matchmaking service_tests + 手工联调。

## 开始实现前门禁

- [x] 用户批准规划并 `task.py start`。
- [x] 切分支并实现阶段 1–2 + 文档。
