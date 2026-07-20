# 首页与大厅重设计 — 实施计划

## 实施边界

- 仅 `apps/extraction-client` UI：`src/ui/view.ts`、`src/styles/*`；必要时微调 `shell.ts` 类名。
- 不改服务端、不改 controller 业务逻辑（除非 class 钩子必须）。
- 分支建议：`codex/extraction-home-lobby-redesign` from 当前 HEAD。

## 阶段 0：基线

- [x] 切分支 `codex/extraction-home-lobby-redesign`；本地 Vite 可开。
- [x] 未登录 1280×720 截图对照（`apps/extraction-e2e/test-results/home-redesign/unauth.png`）。

## 阶段 1：Token 与壳层

- [x] 更新 `tokens.css`：近黑白极简 + 主 CTA 高对比；保留资源色变量。
- [x] 调整 `shell.css`：顶栏细线、scene-shade 渐变、侧栏对齐。
- [x] 队列面板背景对齐极简；按钮继承 primary/secondary。

## 阶段 2：未登录结构

- [x] 改写 `renderAuth`：身份一行化；hero 主按钮；底栏 server-status。
- [x] `panels.css`：`.access-panel` / `.auth-identity-row` / hero 按钮。
- [x] DEV 单机入口样式协调且仍 `import.meta.env.DEV`。

## 阶段 3：大厅结构

- [x] 改写 `renderLobby` / `renderWarehouse` 分区与 class。
- [x] 三资源大数字 + 小色点；`warehouse-stats` 次要；`recent-result` 底部。
- [x] 主 CTA「开始匹配」hero 全宽。

## 阶段 4：响应式与无障碍

- [x] `responsive.css`：窄宽侧栏；hero 高度；资源数字缩放。
- [x] reduced-motion 既有规则保留。

## 阶段 5：验证

```bash
pnpm --filter @voxelize/extraction-client test -- src/ui/view
pnpm --filter @voxelize/extraction-client typecheck
```

- [x] `view.test.ts` 无需改（文案断言仍过）；全量 client 测试 293 passed。
- [x] typecheck 通过。
- [ ] 用户目视大厅（需登录）与最终 AC 勾选。

## 回滚点

- 阶段 1 仅 CSS 可独立 revert。
- 阶段 2–3 的 `view.ts` 与 CSS 同批回滚。

## 开始实现前门禁

- [x] 用户批准 PRD + design + implement。
- [x] `task.py start 07-20-extraction-home-lobby-redesign`
