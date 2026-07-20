# 首页与大厅重设计 — 技术设计

## 目标边界

- **改**：`tokens` 高对比极简色板；`shell` 顶栏/侧栏框架微调；`panels` 登录/大厅分区；`view.ts` 未登录与大厅 DOM 结构（保留 `data-action`）。
- **不大改**：`ProductController` 状态机、API、匹配协议、局内 HUD 逻辑。
- **禁**：伪造数据、生产 DEV 入口、Web3 霓虹视觉。

## 布局

```
┌──────────────────────────────────────────────────────────┐
│ topbar: brand (左)              account chips (右)       │
├──────────────────────────────────────┬───────────────────┤
│                                      │  side rail        │
│         world-canvas (full)          │  (unauth | lobby) │
│                                      │  固定宽 ~400–440  │
│                                      │  分区垂直堆叠     │
└──────────────────────────────────────┴───────────────────┘
```

- `screen-content` 仍 `display:grid`；子面板 `justify-self:end; width:min(420px,100%); height:100%`。
- 场景遮罩略减（极简下避免灰雾过重），保证侧栏边缘清晰。

## 视觉 token（高对比极简）

| Token | 方向 |
|-------|------|
| `--color-void` | 近黑 |
| `--color-surface` | 近黑半透明侧栏（少 blur 或轻 blur） |
| `--color-ink` | 近白 |
| `--color-ink-muted` | 中灰 |
| `--color-line` | 细浅灰线，少发光 |
| `--color-action` | **高对比白底黑字或黑底白字主按钮**（主 CTA 实心，非柔薄荷绿主导） |
| 资源色 | 保留 dirt/gold/diamond 仅用于 `.resource-cube` 小点 |

主 CTA：全宽、更高 `min-height`（~52px）、字重 600+；次按钮描边无填充。

## DOM / 组件契约

### 未登录 `renderAuth`

```
.access-panel.side-rail
  .panel-heading (kicker + h1 + .rule-line)
  .auth-identity-row (一行状态，非大卡)
  button[data-action=connect-wallet].primary-command.primary-command--hero
  (DEV) a.single-player-entry
  .server-status (mt:auto)
```

### 大厅 `renderLobby`

```
.lobby-panel.side-rail
  .panel-heading
  .lobby-actions (join-queue hero + refresh secondary)
  .warehouse-section
    .resource-balances (三列：cube + 名 + 大数字)
    .warehouse-stats (次要，2×2)
  .recent-result (mt:auto 或紧贴统计下)
```

- `renderWarehouse` 导出保持；测试断言的 `累计撤离资源` 等文案保留。
- 所有现有 `data-action` 字符串不变。

## 兼容与回归

| 区域 | 策略 |
|------|------|
| 队列/结果/连接中面板 | 继承 token 与按钮类名，结构不动 |
| `view.test.ts` | 更新选择器仅当结构必须变；优先保文案断言 |
| 响应式 | `responsive.css`：窄屏侧栏可全宽叠底或减 padding，CTA 不丢 |

## 风险

| 风险 | 缓解 |
|------|------|
| 极简导致「空」 | 用字号阶梯与 1px 分割线分区，不用装饰插画 |
| 主按钮过白刺眼 | 用实心 ink/void 对，hover 仅明度阶，无霓虹 |
| 共享 token 波及 HUD | HUD 颜色独立用 resource/health token；主 action 变更后目视局内一次 |

## 回滚

仅客户端 CSS/HTML 字符串；`git revert` 单 commit 即可恢复。
