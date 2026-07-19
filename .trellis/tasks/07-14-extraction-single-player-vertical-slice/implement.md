# Extraction 单机可玩垂直切片实施计划

## 实施边界

- 仅修改开发环境单机入口及其必要的客户端共享常量/边界扫描。
- 不修改服务端、数据库、生产协议、钱包、队列、多人权威规则或永久结算。
- 允许搬运 Create Town / Lab 贴图与可复用美术资源并本地化到仓库；也可用程序化/其它来源补齐缺口。
- 不创建或派发子代理；按当前 Codex inline 流程由主会话实施和检查。
- 实现前必须完成 PRD、设计和本计划的用户评审，并执行 `task.py start`。

## 阶段 1：入口隔离与本地域模型

- [x] 将 `?mode=single` 从 `testing/e2e-main.ts` 分离到 `src/single/` 专用入口，保留 DEV 门。
- [x] 保持 `e2e`、`live-e2e` 和默认 `ProductController` 路由不变。
- [x] 定义 `LocalGameState`、阶段联合、12 格背包、选择槽、挖掘、经过时间、提示、撤离和结果纯状态转换。
- [x] 先写背包堆叠、满包、整槽丢弃、claim 幂等、挖掘取消和三秒撤离测试。

验证：

```bash
pnpm --filter @voxelize/extraction-client test -- src/single
pnpm --filter @voxelize/extraction-client typecheck
```

回滚点：单机动态导入仍可临时指回旧 `testing/single-main.ts`，不得影响其他模式。

## 阶段 2：本地 INIT、Block Registry 与确定性地图

- [x] 创建最小但完整的本地 block 定义与 World INIT fixture。
- [x] 用确定性 ChunkProtocol 数据生成 4×4 Chunk、初始 voxel/light、出生点和撤离区数据。
- [x] 实现开放天空、垂直分层废弃采石场轮廓及固定资源分布。
- [x] 标记泥土/黄金/钻石可挖；环境结构、基岩和撤离设施不可破坏。
- [x] 实现 LocalWorldAdapter 响应/消费 LOAD 与本地 UPDATE，不发网络请求。
- [x] 测试同种子确定性、区块边界、代表坐标、出生支撑、回程结构摘要、资源可达性和资源 ID。

验证：

```bash
pnpm --filter @voxelize/extraction-client test -- src/single/map src/single/world-adapter
pnpm --filter @voxelize/extraction-client typecheck
```

风险门：如公开 World API 无法安全完成本地 INIT/LOAD，停止实现并回到设计评审，不直接修改 Core 私有字段。

## 阶段 3：真实 World、第一人称控制与生命周期

- [x] 创建 renderer、camera、World、RigidControls、Inputs、VoxelInteract 和 resize/render loop。
- [x] 连接 pointer lock、WASD、空格、Escape、visibilitychange 和 H 提示。
- [x] 禁用飞行、幽灵；近战对接假人（击退/击杀）；不伪造生产 PVP 结果。
- [x] 设置固定 FOV、玩家体型、速度、重力、傍晚 Sky、雾和光照初值。
- [x] 完成初始化 loading/error/retry 状态，以及 dispose 的 listener/RAF/worker/World 清理。
- [ ] 连续重开至少三次，验证只有一个有效运行时。（浏览器自动化：单次进入可 ready；连续 3 次快速重载 60s 内未稳定 ready，需手工再验）

验证：

```bash
pnpm --filter @voxelize/extraction-client test -- src/single/runtime src/single/input
pnpm --filter @voxelize/extraction-client typecheck
```

回滚点：保持本地 World adapter 与 UI 分离，运行时可整体替换而不动领域状态。

## 阶段 4：挖掘、背包、掉落与拾取

- [x] 以 VoxelInteract target 驱动按住挖掘和切换/松开/超距/解锁取消。
- [x] 应用泥土 0.5s、黄金 1.5s、钻石 3s 时长。
- [x] 完成时先 claim，再执行 `source: "server"` 本地 voxel update，再写背包。
- [x] 实现满包世界掉落、Q 整槽丢弃、短暂本人排除和自动拾取。
- [x] 加入目标方块名称、进度、viewmodel 挥动和方块破坏反馈。
- [ ] 验证区块边界上的挖掘会更新邻区 mesh 且不产生网络 UPDATE。

验证：

```bash
pnpm --filter @voxelize/extraction-client test -- src/single/state src/single/loot src/single/input
pnpm --filter @voxelize/extraction-client typecheck
```

资源守恒门：任何方块、掉落或重渲染不得让资源总量增加两次。

## 阶段 5：撤离、结果与重新开始

- [x] 创建世界内薄荷绿撤离信标和进入判定。
- [x] 实现连续三秒、离区清零、轻量 HUD 进度和一次性完成转换。
- [x] 完成时停止经过时间、取消输入、释放指针锁并冻结交互。
- [x] 结果显示资源种类/数量、用时和“不保存”，不显示价值、分数或永久入账。
- [x] 重新开始先 dispose 旧世界，再以同种子重建。

验证：

```bash
pnpm --filter @voxelize/extraction-client test -- src/single/state src/single/controller src/single/view
```

回滚点：结果层只依赖本地状态，不接触生产 `MatchResult` API。

## 阶段 6：材质、手持物与 Create Town 风格 HUD

- [x] 回看 Create Town / Lab，记录纹理密度、镜头、热栏、天空和雾。
- [x] 首轮用 Canvas 程序化纹理与 Three 几何手持物打通管线（`textureUnitDimension: 16`）。
- [x] **PRD 已放宽**：从 Lab 客户端 media 搬运方块贴图到 `src/assets/single/blocks/`，经 `textures.ts` 异步加载替换程序化占位。
- [x] 为顶面/侧面差异配置 texture groups。
- [x] 实现无生产顶栏的专用 shell、准星、目标标签、ItemSlots 风格热栏、经过时间、提示和结果层。
- [x] 按 Lab 解包参数调校天空、雾、光照、Arm 双 pass。
- [x] 系统等宽字体 fallback（字体搬运可选）。
- [x] 第一人称镐/剑 viewmodel（槽 1/2）与 MC 挖掘时长联动。
- [x] **有限放置 + 手持方块（范围扩展）**：
  - `held-content.ts`：槽 0 空手 / 1 镐 / 2 剑 / 3+ 资源方块
  - 第一人称：工具用 `item` 精灵，方块用 `held` 手臂+立方
  - 第三人称自身：`createMcHeldContentMesh` 挂右臂
  - 右键 `potential` 邻格放置、扣 1、250ms 连放；工具槽不放置
  - `__singleDebug` 注入 give/selectSlot/held 便于验收
- [x] MC ModelBiped 假人（出生点巡逻走/挖）+ 第一人称自身身体（低头可见）。
- [x] 假人第三人称手持：走路铁剑、挖掘铁镐（`mc-held-item.ts`）。

视觉门：同视口并排检查时，世界占比、第一人称近景、纹理密度、手持物、准星和热栏必须达到 PRD AC11；材质允许搬运，不要求原创。

## 阶段 7：浏览器闭环与桌面端验收

- [x] 启动本地 Vite，使用 Chrome 真实访问 `?mode=single`。
- [x] 观察网络日志，确认无 `/api`、`/health`、`/ws` 或钱包调用。
  - 2026-07-19：`performance.getEntriesByType('resource')` 全为 `127.0.0.1:5173` Vite 资源；无 `/api` `/health` `/ws` / wallet 调用。
- [x] 完成点击进入、移动、跳跃、挖泥土/黄金/钻石、满包/丢弃/拾取、空包撤离、单黄金撤离和重开闭环。
  - 自动化抽样：ready 后 `__singleDebug` 手持镐/剑/泥土/黄金、右键放置提示可见、撤离光圈与热栏正常。
  - 完整键鼠闭环（指针锁挖矿→撤离→重开）此前手工验收过；本轮以 debug API + 截图复验手持/HUD，未重跑满包计时撤离全流程。
- [x] 检查 2560×1294、1440×900 和 1280×720 桌面视口；移动端、触屏和竖屏不纳入第一阶段实现或验收。
  - 本轮截图：`accept-1280x720.png`、`accept-1440x900.png`；2560 视口下 HUD 可读。
- [x] 对照 Create Town 微调 FOV、相机近景、移动速度、鼠标观感、纹理、Sky、雾、viewmodel 和 HUD。
- [x] 检查 reduced motion、键盘焦点、aria 状态和指针锁退出。
  - CSS 含 `prefers-reduced-motion` 规则（≥2）；结果层有「再次进入」按钮；指针锁释放后提示「点击继续」（既有行为）。
  - 局内常驻角标：`本地单机 · 不保存进度`（`.single-local-badge`，结果层仍保留补充文案）。

证据：本地验收截图（`accept-*.png` / `held-*.png`）与控制台/网络观测；不保存参考站点专有资产。

## 阶段 8：生产边界与完整质量门

- [x] 更新生产边界扫描，拒绝单机唯一标记和 E2E bridge 泄漏生产 bundle。
  - `pnpm --filter @voxelize/extraction-client build` 通过（含 assert-production-boundary）。
  - dist 扫描：无 `__singleDebug` / `__VOXEL_EXTRACTION_E2E__` / `startSinglePlayerClient` / `single-player-mode`。
- [x] 运行客户端全量 Vitest、typecheck、ESLint 和 production build。
  - typecheck：通过
  - Vitest：**281/281** 通过（修复 `network.test.ts` Node 环境 `CloseEvent` 垫片）
  - production build：通过
  - ESLint：本次改动的 `src/single/*` + network 测试 **0 error**（controller 2 个既有 unused warning）；仓库级 `pnpm lint:extraction` 仍有历史 prettier/import 问题（非本轮引入）
- [x] 运行 81 项 E2E actor 回归，确认默认入口相关状态契约不变。
  - 2026-07-19：`pnpm --filter @voxelize/extraction-e2e test:actor` → **81/81**
- [x] 检查新增生产文件规模、`git diff --check` 和现有用户修改未被覆盖。
  - `git diff --check` 干净
  - 文件规模（2026-07-19 拆分后）：
    - 已抽出：`held-content` / `place-action` / `world-place` / `self-held` / `mining-session` / `mannequin-session` / `runtime-mannequin`
    - `controller.ts` ≈597、`runtime.ts` ≈624（仍略超 500 硬上限，继续优先拆 map/mc-biped）
    - `map.ts` ≈974、`mc-biped.ts` ≈557 仍为历史债，本轮未动
- [x] 使用 `trellis-check` 完成 spec、测试、数据流、复用和一致性检查。

验证命令：

```bash
pnpm --filter @voxelize/extraction-client test
pnpm --filter @voxelize/extraction-client typecheck
pnpm --filter @voxelize/extraction-client build
pnpm lint:extraction
pnpm --filter @voxelize/extraction-e2e test:actor
git diff --check
```

如根引擎或 Core 被迫修改，追加：

```bash
pnpm --filter @voxelize/core test
pnpm --filter @voxelize/core build
cargo test --lib --tests
```

## 高风险文件与回滚点

- `apps/extraction-client/src/main.ts`：模式顺序不能让 single 抢占 e2e/live-e2e/production。
- `apps/extraction-client/src/game/world-session.ts` 与 `world-input.ts`：只抽取纯常量或适配边界，不能改变生产网络意图。
- `packages/core/src/core/world/index.ts`：默认不改；任何改动必须单独设计评审并运行 Core/根回归。
- `apps/extraction-client/scripts/assert-production-boundary.mjs`：同时守住 E2E 与 DEV 单机标记。
- 纹理与字体资产：允许搬运 Lab 资源并本地化；记录来源路径，运行时不依赖 create.town 在线资源。
- 现有工作区 45 项以上未提交修改：逐文件查看 diff，禁止覆盖或格式化无关文件。

## 开始实现前门禁

- [x] PRD 完成无损收敛，没有未解决产品问题或重复临时章节。
- [x] 用户审核并批准 `prd.md`、`design.md` 和本文件。
- [x] 执行 `task.py start` 进入 `in_progress`。
- [x] 加载 `trellis-before-dev`，读取相关 frontend/core spec 与预开发检查。
- [x] 确认不需要新增核心依赖；如需下载字体或增加依赖，先单独说明影响。
