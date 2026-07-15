# 浏览器验收笔记 — 2026-07-15

环境：Chromium via Playwright MCP，`http://127.0.0.1:5173/?mode=single`，视口 1440×900。
Vite：`pnpm --filter @voxelize/extraction-client dev`。

## 通过

| 项 | 证据 |
|---|---|
| DEV 单机入口 | `?mode=single` 进入专用 shell，phase→playing |
| 无业务网络 | performance/network 无 `/api` `/health` `/ws` wallet/reown |
| 世界加载 | 首次进入后 `data-single-world=ready`，可见地形/天空/雾 |
| 指针锁 | 点击 canvas 可 lock；工具键 1/2/3 切换 hand/pickaxe/sword |
| 移动 | WASD 后 `data-single-position` 变化 |
| 第一人称手持 | 可见 Arm 双 pass；镐/剑热栏图标正确 |
| MC 假人 | 出生点旁 Steve 可见，手持物（剑/镐阶段）可见 |
| 第一人称自身身体 | 低头可见腿/躯干（截图 `single-sword-locked.png`） |
| 场景生物 | 可见粉色动物实体 |
| 地图风格 HUD | 如「荒原 · 血月」「雪原 · 寒夜」 |

截图（Playwright 输出目录）：
- `single-ready-spawn.png` — 就绪出生点、假人、镐手臂
- `single-sword-after-move.png` — 假人持剑、动物
- `single-sword-locked.png` — 低头自身腿 + 假人

## 未完成 / 阻塞

| 项 | 说明 |
|---|---|
| 完整挖掘闭环 | 自动化瞄准 VoxelInteract 目标困难；CDP 相对鼠标后 target 常为空；未验证泥土/金/钻掉落 |
| 满包/丢弃/拾取 | 依赖挖掘产出，未跑 |
| 撤离三秒 + 结果层 | 未跑到信标 |
| 「再次进入」重开 | 依赖结果层按钮；连续 3 次 `goto` 重载在 60s 内未稳定 ready（pos 常 null） |
| reduced motion / aria | 未系统检查 |
| 区块边界 mesh | 未验 |

## 观察 / 风险

1. **首次 ready 偏慢**：有时 20–40s+ 才从 loading→ready；期间画面可能已渲染但 HUD 仍显示「正在塑造废弃采石场」。
2. **连续重载不稳定**：快速 3 次导航后长时间停在 loading 且 `singlePosition` 为空，疑似 initialize/RAF/WebGL worker 在重载压力下未推进；需手工 Chrome 验证是否仅自动化环境问题。
3. **Pointer Lock**：unlock 后再 lock 时出现 `Unable to use Pointer Lock API`（自动化手势限制）；真实用户点击通常可恢复。
4. **唯一控制台业务外错误**：`favicon.ico` 404；扩展注入 ethereum 报错可忽略。
5. **页面曾 crash**（WebGL 压力后 Target crashed），复现不稳。

## 建议下一步

1. 手工 Chrome 走完：挖泥土/金/钻 → 满包/Q/拾取 → 撤离 → 再次进入×3。
2. 若手工也慢 ready：查 `isWorldReady` 邻域 chunk 与 mesher 队列。
3. 若第一人称身体遮挡准星体验差：考虑 self body `layers` 或 raycast 忽略。
