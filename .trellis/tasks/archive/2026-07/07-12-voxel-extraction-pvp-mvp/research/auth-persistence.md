# 认证与持久化研究

## 外部依据

- Reown AppKit JavaScript SIWE 文档：`https://docs.reown.com/appkit/javascript/core/siwe`
- Reown AppKit JavaScript 安装文档：`https://docs.reown.com/appkit/javascript/core/installation`
- Rust SIWE crate：`https://crates.io/crates/signinwithethereum`
- Rust SIWE 官方库说明：`https://github.com/signinwithethereum/docs/blob/main/docs/libraries/rust.mdx`

2026-07-12 核验结果：

- Reown 客户端契约需要 `getNonce`、`verifyMessage`、`getSession` 和 `signOut`，并明确会话来自后端；
- 当前纯 TypeScript 客户端适合 Reown Ethers v6 adapter，不需要引入 React/Wagmi；
- Rust `signinwithethereum 0.8` 是旧 `siwe 0.6` 的活跃继任者；
- 启用 `alloy` feature 后可验证 EIP-1271 和 EIP-6492 智能钱包；EOA 的 EIP-191 验证可本地完成，智能钱包验证需要 Ethereum RPC；
- RPC 只参与需要链上合约签名验证的登录，已有会话、实时比赛和链下结算不依赖 RPC。

## 依赖建议

服务端应用独立 Cargo manifest 使用：

- `sqlx`，启用 PostgreSQL、Tokio、TLS、UUID、时间与 migration 支持；
- `signinwithethereum` 0.8，启用 `serde`、`alloy`；
- `uuid`、`serde`、`time`/`chrono`、密码学安全随机数；
- 不把这些业务依赖加入根 `voxelize` crate。

客户端应用使用：

- `@reown/appkit`；
- `@reown/appkit-adapter-ethers` 与 `ethers`；
- `@reown/appkit-siwe` 与 `siwe`；
- `mainnet` 从 `@reown/appkit/networks` 导入；
- Reown Project ID 和服务端 URL 通过构建环境注入。

依赖版本在实施时锁定并审计，不顺带升级根项目无关依赖。

## HTTP 与会话契约

同源部署优先，浏览器通过 `Secure + HttpOnly + SameSite=Lax` 的随机不透明 cookie 持有会话，长期 token 不进入 WebSocket URL。

```text
GET  /api/auth/siwe/nonce
POST /api/auth/siwe/verify  { message, signature }
GET  /api/auth/session
POST /api/auth/logout
GET  /api/warehouse
POST /api/matchmaking/queue
DELETE /api/matchmaking/queue
GET  /api/matches/{match_id}/result
GET  /api/matches/latest-result
```

验证顺序：

1. 读取并解析 EIP-4361 消息；
2. 从数据库锁定 nonce，检查未消费且未过期；
3. 对照配置 allowlist 验证 scheme、domain、URI、version、chain ID `1`、issued-at、not-before 和 expiration；
4. 验证签名与消息地址一致；智能钱包使用带超时的 Mainnet RPC；
5. 在同一事务中消费 nonce、查找或创建账号/钱包凭证、撤销冲突旧会话并创建新会话；
6. 只保存会话 token 的密码学哈希；明文只通过 HttpOnly cookie 返回一次；
7. WebSocket 握手通过通用 authenticator 查询会话并得到 `ConnectionPrincipal { account_id, session_id }`。

失败时不得消费之外的业务数据，不记录完整消息、签名、nonce、cookie、RPC 凭证或数据库口令。错误响应使用稳定错误码，不暴露验证内部细节。

钱包地址以规范化 20-byte 值建立唯一约束，显示时再格式化；比赛、仓库和流水始终以不可变 `account_id` 作为主键。AppKit 发现地址、网络变化或断开时调用 `/api/auth/logout` 并清空本地游戏控制状态，重新完成 SIWE 后才能继续。服务端无法感知未上报的本地钱包 UI 变化；安全边界是已验证会话的显式撤销和过期。

## PostgreSQL 最小 schema

### 身份

- `accounts(id uuid pk, created_at, status)`
- `wallet_credentials(account_id fk, chain_id bigint, address bytea, created_at, unique(chain_id,address))`
- `auth_nonces(id uuid pk, nonce_hash bytea unique, domain, uri, expires_at, consumed_at, created_at)`
- `auth_sessions(id uuid pk, token_hash bytea unique, account_id fk, chain_id, address, expires_at, revoked_at, created_at, last_seen_at)`

### 比赛

- `matches(id uuid pk, state, world_name unique, seed, generation_version, gameplay_version, config_version, started_at, extraction_open_at, hard_deadline, finished_at, abort_reason)`
- `match_participants(match_id fk, account_id fk, seat_id smallint, state, reconnect_deadline, killed_by_account_id, extracted_at, mined_counts jsonb, pickup_counts jsonb, lost_counts jsonb, primary key(match_id,account_id), unique(match_id,seat_id))`

局内背包、生命、掉落和挖掘进度不持续写数据库；它们属于 Match World 内存。进程崩溃时按产品规则作废未提交资产。

### 永久资产

- `extraction_settlements(id uuid pk, match_id fk, account_id fk, inventory_digest bytea, config_version, total_value bigint, committed_at, unique(match_id,account_id), check(octet_length(inventory_digest)=32))`
- `settlement_items(settlement_id fk, item_key text, quantity bigint check(quantity>=0), primary key(settlement_id,item_key))`
- `warehouse_balances(account_id fk, item_key text, quantity bigint check(quantity>=0), primary key(account_id,item_key))`
- `asset_ledger(id uuid pk, account_id fk, settlement_id fk, item_key text, delta bigint check(delta>0), created_at, unique(settlement_id,item_key))`

稳定 `item_key` 只有 `dirt/gold/diamond`。永久表不保存依赖注册顺序的运行时数值 ID。累计数量、价值、成功撤离次数和最高单局收益从 settlement/ledger 聚合，避免维护第二份可漂移统计真相。

所有状态列使用数据库 CHECK 或受控枚举值；时间使用 UTC；数量使用足够宽的非负整数；常用账号、比赛、会话过期和流水查询建立索引。

## 撤离事务

World 达到连续 8 秒后先把参与者转为 `SettlementPending`，冻结生命、背包、攻击、拾取和移动，再把不可变 payload 发送到有界异步 worker。ECS tick 不等待数据库 I/O。

```text
BEGIN
  INSERT extraction_settlements(match_id, account_id, ...)
    ON CONFLICT(match_id, account_id) DO NOTHING

  若已存在：读取原 settlement/items，digest 相同则返回原结果；不同则报警并拒绝
  若新建：
    INSERT settlement_items
    INSERT asset_ledger（unique settlement_id + item_key）
    UPSERT warehouse_balances SET quantity = quantity + excluded.quantity
    UPDATE match_participants SET state = 'extracted', extracted_at = ...
COMMIT
```

commit 成功后 World 才进入 `Extracted` 并向客户端确认。超时或可重试错误继续使用同一 settlement ID；进程在 commit 后、响应前崩溃时，重启核对返回原结果而不再次加仓。事务失败/回滚或 commit 前崩溃不入账。

比赛异常终止只把未终态参与者标记失败/异常，不为局内背包创建 settlement。已经 commit 的 settlement 不回滚。

## 内部只读运维

不开发可视化后台。建议提供单独 CLI 或独立内网 listener，只开放：

- 按 address/account_id 查询账号；
- 查询 match/participant；
- 查询 settlement/items；
- 查询 warehouse projection 和 asset ledger。

运维使用独立只读数据库角色或只读 repository，限制网络、分页和速率，并记录不含秘密的访问日志。不存在余额修改、settlement 重放、任意 SQL、nonce/session 明文查询接口。

## 测试与迁移

- SIWE：标准 EIP-191 向量、错误签名、chain/domain/URI、过期、not-before、重复 nonce、地址切换；
- 智能钱包：mock RPC 的 EIP-1271/EIP-6492 成功、拒绝、超时；RPC 失败不得降级跳过验证；
- Session：cookie 属性、撤销、过期、重复连接、WebSocket 不接受伪造 client_id；
- SQLx：并发 nonce 消费只成功一次，并发相同 settlement 最终只入账一次；
- 故障注入：commit 前崩溃、事务回滚、commit 后响应前崩溃、重启核对；
- migration 只做 additive。对非一次性数据库执行 migration 前必须按危险操作规则另行确认，不提供会删除账本数据的自动 down migration。
