# Extraction 首页与大厅重设计

## Goal

统一重设计未登录首页与已登录行动大厅：右侧战术侧栏 + 高对比极简视觉，主 CTA 明确，仓库扫读友好，且不破坏钱包登录、匹配、权威仓库数据与生产边界。

## Background

### Confirmed Facts

- 壳层：`apps/extraction-client/src/ui/shell.ts` — 全屏 canvas + 顶栏 + 右侧 `screen-content`。
- 渲染：`ui/view.ts` 的 `renderAuth` / `renderLobby` / `renderWarehouse`；样式 `styles/tokens.css`、`shell.css`、`panels.css`。
- 交互：`data-action`（`connect-wallet`、`join-queue`、`refresh-lobby`、`logout`、`show-result` 等）由 `ProductController` 绑定。
- 产品约束：`PRODUCT.md` — 战术、克制、可信；禁 Web3 霓虹营销页；第一屏可操作；服务端事实；游戏画面优先。
- DEV 单机入口受 `import.meta.env.DEV` 门控。

### Problem Restatement

右侧面板信息正确但层次扁平：主 CTA 与仓库权重接近，登录/大厅衔接弱，缺乏高对比极简下的清晰指挥台感。

### Fundamental Constraints

- 不伪造仓库/匹配/结果；数字来自服务端快照。
- 不改协议与匹配规则；以客户端 UI 为主。
- 生产包不泄漏 DEV 入口。
- 桌面优先；键盘焦点与对比度达标；状态不全靠颜色。

## Requirements

### R1. 布局

- 保持**右侧战术侧栏**全高操作台；左侧/背景为体素世界。
- 未登录与大厅共用同一侧栏宽度、顶栏、按钮与分区语言。

### R2. 视觉

- **高对比极简**：近黑白主 UI、大字重主 CTA、少阴影/少装饰动效。
- 资源行：**中文标签 + 小色点 + 大数字**（泥土/黄金/钻石点缀色仅作辅助识别）。
- 动效默认接近无；尊重 `prefers-reduced-motion`。

### R3. 未登录

- 顺序：kicker/标题 → 规则单行（10 人 / 300×300 / 12 分钟）→ 主登录按钮 →（DEV）单机链 → 底栏服务状态。
- 身份：一行 chip/文案，非大卡片。

### R4. 大厅

- 顺序：标题区 → **开始匹配**（主）+ 刷新（次）→ 三资源大数字 → 最近行动一行 → 生涯四项统计（次要字号/两列）。
- 匹配按钮视觉权重高于仓库与统计。

### R5. 兼容

- 保留全部既有 `data-action` 与测试语义（`renderWarehouse` 统计文案等可小改结构但断言字段保留）。
- 队列/结果/HUD 仅共享 token 微调，不整页重做。

## Acceptance Criteria

- [ ] AC1：未登录与大厅同一套 token/侧栏组件语言（黑白极简 + 资源点缀）。
- [ ] AC2：未登录可走通连接/签名登录；`data-action` 兼容。
- [ ] AC3：大厅可匹配、刷新；仓库三资源与四项统计仍展示服务端值。
- [ ] AC4：`view` 相关测试 + client typecheck 通过；生产 build 无 DEV 单机入口泄漏。
- [ ] AC5：1280×720 下侧栏无重叠，主 CTA 始终可见且可点。

## Out of Scope

- 匹配规则、链上资产、大厅 3D 定制、完整 HUD/结果页重做、i18n 体系、营销落地页。

## Decisions

| 项 | 决定 |
|----|------|
| 范围 | 未登录 + 大厅 |
| 布局 | 右侧战术侧栏 |
| 气质 | 高对比极简 |
| 大厅 IA | 匹配优先 → 三资源 → 最近行动 → 次要生涯统计 |
| 未登录 IA | 短规则条 + 一行身份 + 主按钮 + 底栏服务状态 |
| 资源色 | 标签 + 小色点 + 大数字 |
| 动效 | 默认几乎无（规划默认，不单独立项） |
