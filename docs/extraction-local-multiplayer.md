# Extraction 本地多人联调

在 **DEV 可配置成局人数**（默认 2）下，用两个浏览器 + 真实 SIWE 钱包验证：匹配、同局互见、权威挖矿/近战、撤离或死亡结果。

生产路径仍为 **10 人**；未设置 `EXTRACTION_DEV_MATCH_MODE` 时行为与上线一致。

## 前置

- Postgres（例：`postgres://localhost/voxelize_extraction`）
- Node/pnpm、Rust（`cargo`）
- 两个以太坊钱包（或两个浏览器 Profile 各连一个钱包）
- Reown Project ID（客户端 `.env`）

## 服务端

```bash
# 1. 数据库（首次）
createdb voxelize_extraction   # 或等价
# migrate 随服务启动或按仓库 migrations 说明执行

# 2. 环境变量
cp apps/extraction-server/.env.example apps/extraction-server/.env
# 编辑 DATABASE_URL，并开启 DEV 成局：
# EXTRACTION_DEV_MATCH_MODE=true
# EXTRACTION_DEV_MATCH_SIZE=2

# 3. 启动（需 engine feature）
cd apps/extraction-server
# 确保加载 .env（direnv / export $(cat .env | xargs) 等）
cargo run --features engine
```

默认监听：`http://127.0.0.1:4100`  
健康检查：`GET /health/ready`（需带 `Origin: http://127.0.0.1:5173` 或经 Vite 代理）  
同一 `DATABASE_URL` 只能有一个 matchmaking 进程（进程锁）。

协议层双人冒烟（无需浏览器钱包 UI）：

```bash
# server 已开 DEV_MATCH_MODE 后
node apps/extraction-e2e/scripts/dev-two-player-smoke.mjs
```

协议层完整玩法（挖矿 → 互见 → 近战击杀 → 死亡掉落/拾取）：

```bash
cd apps/extraction-e2e
EXTRACTION_E2E_SERVER_URL=http://127.0.0.1:4100 \
EXTRACTION_E2E_CLIENT_URL=http://127.0.0.1:5173 \
EXTRACTION_E2E_PUBLIC_ORIGIN=http://127.0.0.1:5173 \
pnpm exec vitest run --config vitest.actor.config.ts src/live/dev-mp-gameplay.actor.ts
```

浏览器双独立 context 进局（避免同 profile 共享 cookie）：

```bash
node apps/extraction-e2e/scripts/dev-two-browser-smoke.mjs
```

> 上一局结束后约 1 分钟（reconnect timeout）才释放座位；连跑失败见 `MATCH_FULL` 时稍等再试。

## 客户端

```bash
cp apps/extraction-client/.env.example apps/extraction-client/.env
# VITE_REOWN_PROJECT_ID=...  # 仅正式钱包入口需要；DEV 多人可跳过

pnpm --filter @voxelize/extraction-client dev
```

### 推荐：跳过钱包 UI（局内表现联调）

服务端已开 `EXTRACTION_DEV_MATCH_MODE=true` 后，开两个窗口：

| 窗口 | URL |
|------|-----|
| A | `http://127.0.0.1:5173/?mode=dev-mp&seat=0` |
| B | `http://127.0.0.1:5173/?mode=dev-mp&seat=1` |

- 自动确定性 SIWE（无 MetaMask）
- 自动点「加入匹配」
- 顶栏显示 `DEV 多人 · seat N`

### 正式钱包路径（验收用）

打开：`http://127.0.0.1:5173/`（**不要** `?mode=single` / `dev-mp`）  
SIWE 域须与 `EXTRACTION_SIWE_DOMAIN` 一致（默认 `127.0.0.1:5173`）。

## 双人流程

1. 两个窗口分别进 `dev-mp&seat=0/1`（或正式钱包登录）  
2. 入队后满 N=2 成局，进入同一体素世界  
3. 验收清单：
   - [x] 能看到对方角色（不同配色 + 名牌）— 协议 PEER 互见已自动化  
   - [x] 挖掘泥土/矿并出现权威背包或掉落  
   - [x] 近战造成服务端确认的伤害 → 击杀  
   - [x] 死亡掉落进入击杀者背包（贴脸可能秒吸，实体帧可不稳）  
   - [ ] 撤离成功（本切片玩法脚本未覆盖；AC5 由死亡掉落路径满足）  

## 常见问题

| 现象 | 处理 |
|------|------|
| 一直排队 | 确认 `DEV_MATCH_MODE=true` 且第二人已入队；或 `DEV_MATCH_SIZE` 是否被设为更大 |
| 进程起不来 / 锁占用 | 杀掉其它 extraction-server；检查 `DATABASE_URL` |
| SIWE 失败 | 检查 domain/uri/cookie；用 `127.0.0.1` 不要混用 `localhost` |
| WS 连不上 | 确认 API origin、代理与 cookie 同站策略 |

## 关闭 DEV 成局

去掉 `EXTRACTION_DEV_MATCH_MODE` 或设为 `false` 后重启服务 → 恢复 **10 人** 成局。
