# Minecraft 原版挖掘设计解包

> 日期：2026-07-15  
> 范围：Java Edition 挖掘时间模型 + 与单机 Extraction 对照  
> 证据来源：
> - [minecraft.wiki/Breaking](https://minecraft.wiki/w/Breaking)（2026-07-08 修订）
> - `minecraft-data@3.90.0` → `pc/1.21.4/blocks.json` / `materials.json`（PrismarineJS 解包数据，非 jar 逆向）
> - create.town/lab 当前 HTML 入口脚本扫描（无服务端硬度表）

## 1. 原版核心模型（三要素）

挖掘时间由三件事决定：

1. **方块 hardness**（硬度，浮点数）
2. **当前手持物**是否为**正确工具**、是否为**最佳工具类型**、工具材料等级
3. **玩家惩罚/增益**（水下、潜行跳、急迫、疲劳、效率附魔等）

原版**不是**「固定 ms 表 × 简单倍率」一种表就完事，而是：

```
每 tick 进度 = destroySpeed / hardness / (canHarvest ? 30 : 100)
ticks  = ceil(1 / 进度)   // 进度≥1 则瞬间破坏
秒数  = ticks / 20
```

其中 `destroySpeed`（速度倍率）大致为：

| 情况 | destroySpeed |
|------|----------------|
| 徒手 / 非最佳工具 | 1 |
| 木工具 | 2 |
| 石工具 | 4 |
| 铜工具 | 5 |
| **铁工具** | **6** |
| 钻石 | 8 |
| 下界合金 | 9 |
| 金 | 12 |
| 剑（通用） | 1.5（对蛛网/竹等另有特判） |
| 正确工具 + 效率附魔 | `speed + level² + 1` |

额外：

- **能否收获（canHarvest）**：错误工具破坏石头等会变慢（分母 100 而非 30），且**常常不掉落**。
- **最佳工具类型**：镐→石头/矿；铲→土/沙；斧→木；锄→部分植物。
- **剑不是通用加速镐**：对石头几乎不比手强；在创造模式持剑甚至不能破坏。
- **基岩** hardness = -1，生存不可破坏。

## 2. 1.21.4 数据摘录（minecraft-data）

| 方块 | hardness | material（工具标签） | diggable | 备注 |
|------|----------|----------------------|----------|------|
| dirt | 0.5 | mineable/shovel | true | 铲加速；手可掉落 |
| grass_block | 0.6 | mineable/shovel | true | |
| oak_planks / oak_log | 2.0 | mineable/axe | true | 斧加速；手可掉落 |
| stone | 1.5 | mineable/pickaxe | true | **需镐才掉落** |
| cobblestone | 2.0 | mineable/pickaxe | true | |
| gold_ore / diamond_ore | 3.0 | incorrect_for_wooden_tool | true | 需铁镐及以上才掉落 |
| deepslate | 3.0 | mineable/pickaxe | true | |
| bedrock | -1 | default | **false** | 不可挖 |
| obsidian | 50 | incorrect_for_wooden_tool | true | 需钻石镐 |

`materials.json` 只标记「哪些物品 id 属于该 mineable/* 标签」，**材料等级速度在客户端/物品组件里**，不在 materials 的 1.0 占位里。

## 3. 按公式算出的典型时间（无附魔/无状态效果）

| 方块 | 工具 | 约耗时 |
|------|------|--------|
| dirt | 手 | **0.75s** |
| dirt | 铁铲 | **0.15s** |
| grass | 手 | 0.90s |
| oak_planks | 手 | 3.00s |
| oak_planks | 铁斧 | 0.50s |
| stone | 手（不掉落） | **7.50s** |
| stone | 铁镐 | **0.40s** |
| stone | 剑 | ~7.50s（非镐，慢且通常无掉落） |
| gold/diamond ore | 手 | **15s** |
| gold/diamond ore | 铁镐 | **0.75s** |

规律：原版「正确铁镐 vs 空手挖矿」差距约 **10～20 倍**，不是 3～5 倍。

## 4. create.town/lab 侧

对 `https://create.town/lab` 入口 HTML 中列出的 18 个 JS chunk 全文检索：

- **未发现** `hardness` / `destroyTime` / `miningSpeed` / `breakTime` / `requiresCorrectTool` 等挖掘硬度字段
- 仅有少量 `sword` / `axe` 字符串（物品名/UI）
- Lab/Builder 主玩法是建造放置，**不是**生存挖掘；本次入口包**不能**当作 MC 原版挖掘实现来源

贴图命名（`iron_pickaxe` 等）像 MC，但是**美术资产**，不是挖掘公式。

## 5. 与当前 Extraction 单机设计对照

当前实现（`mining.ts` + `LOCAL_BLOCK_MINING`）：

```
时间 = baseHardnessMs / toolSpeed
```

| 点 | 原版 | 我们现状 |
|----|------|----------|
| 单位 | hardness 浮点 + 20 tick/s | 直接 ms 基准 |
| 工具表 | 木2/石4/铁6/钻8… | 空手1 / 镐 4.2~5 / 剑 1.08~2.3 |
| 错误工具 | 更慢（×100/30）+ 可能不掉落 | 仍可挖，时间略慢 |
| 镐 vs 手挖石 | ~0.4s vs 7.5s（约 18×） | ~0.6s vs 2.6s（约 4×） |
| 矿石 | 硬度 3 + 需铁镐掉落 | 基准 3.2~5.5s，镐约 1s，**手也能掉落** |
| 剑 | 非石/矿加速工具 | 对木有加速，对石几乎无用（接近原版直觉） |
| 基岩 | 不可挖 | 不可挖（一致） |
| 铲/斧 | 独立工具 | **没有铲**；木用剑近似斧 |

## 6. 已实施：严格原版公式（2026-07-15）

实现位置：

- `apps/extraction-client/src/single/mining.ts` — destroySpeed / canHarvest / duration
- `apps/extraction-client/src/single/blocks.ts` — hardness 取自 1.21.4 子集

规则摘要：

1. `ms = round(ceil(1 / (speed/hardness/divisor)) / 20 * 1000)`，`divisor = canHarvest ? 30 : 100`
2. 铁镐 speed = **6**，仅当 `preferredTool === pickaxe`
3. 石/矿 `requiresCorrectToolForDrops`：非镐慢挖（×100）且 **不掉落**
4. 土/草：无铲 → 手/镐/剑均为 speed 1，但可掉落
5. 木梁：无斧 → 手/镐/剑均为 speed 1（3s @ hardness 2）
6. 基岩/信标仍不可挖

## 7. 复现命令

```bash
cd /tmp && npm pack minecraft-data@3.90.0
tar -xzf minecraft-data-*.tgz package/minecraft-data/data/pc/1.21.4/blocks.json
# wiki
# https://minecraft.wiki/w/Breaking
```
