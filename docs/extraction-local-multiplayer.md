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
同一 `DATABASE_URL` 只能有一个 matchmaking 进程（进程锁）。

## 客户端

```bash
cp apps/extraction-client/.env.example apps/extraction-client/.env
# VITE_EXTRACTION_API_URL=http://127.0.0.1:4100   # 若默认代理不足
# VITE_REOWN_PROJECT_ID=...

pnpm --filter @voxelize/extraction-client dev
```

打开：`http://127.0.0.1:5173/`（**不要** `?mode=single`）

SIWE 域必须与 `EXTRACTION_SIWE_DOMAIN` / `EXTRACTION_PUBLIC_ORIGIN` 一致（默认 `127.0.0.1:5173`）。

## 双人流程

1. **Profile A**：登录钱包 A → 进入匹配队列  
2. **Profile B**：登录钱包 B → 进入匹配队列  
3. 队列满 **N=2** 后自动成局，双方进入同一体素世界  
4. 验收清单：
   - [ ] 能看到对方角色  
   - [ ] 挖掘泥土/矿并出现权威背包或掉落  
   - [ ] 近战造成服务端确认的伤害/击退  
   - [ ] 撤离成功 **或** 死亡掉落 其一  

## 常见问题

| 现象 | 处理 |
|------|------|
| 一直排队 | 确认 `DEV_MATCH_MODE=true` 且第二人已入队；或 `DEV_MATCH_SIZE` 是否被设为更大 |
| 进程起不来 / 锁占用 | 杀掉其它 extraction-server；检查 `DATABASE_URL` |
| SIWE 失败 | 检查 domain/uri/cookie；用 `127.0.0.1` 不要混用 `localhost` |
| WS 连不上 | 确认 API origin、代理与 cookie 同站策略 |

## 关闭 DEV 成局

去掉 `EXTRACTION_DEV_MATCH_MODE` 或设为 `false` 后重启服务 → 恢复 **10 人** 成局。
