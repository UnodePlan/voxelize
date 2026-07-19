# Single-player item textures

## Source

Extracted from `https://create.town/lab` Next.js static media:

| Local file | Lab media | Use |
|---|---|---|
| `iron_pickaxe.png` | `iron_pickaxe` (16×16) | 槽 2 / 第一人称镐 / 假人挖掘 |
| `stone_pickaxe.png` | `stone_pickaxe` | 备用 |
| `wood_pickaxe.png` | `wood_pickaxe` | 备用 |
| `iron_sword.png` | `iron_sword` (16×16) | 槽 3 / 第一人称剑 / 假人走路 |
| `wood_sword.png` | `wood_sword` (16×16) | 备用 |

## First-person equip (lab-aligned)

| Hotbar slot (0-based) | Held | Arm API |
|---|---|---|
| 0 | empty hand | `setArmObject(undefined)` → CanvasBox arm |
| 1 | iron pickaxe | plane sprite + `customType: "item"` |
| 2 | iron sword | same as pickaxe, different texture |
| 3+ with qty | resource block | arm+block group + `customType: "held"` |

Inventory rule: slots 0–2 never store resources (`LOCAL_TOOL_HOTBAR_SLOTS` in `state.ts`).  
Runtime:
- 手持内容：`heldContentFromSlot`（`held-content.ts`）
- 第一人称：`LocalViewmodel`（`viewmodel.ts`）
- 第三人称自身/假人：`createMcHeldContentMesh`（`mc-held-item.ts`）
- 有限放置：右键 + 资源槽 → `potential` 邻格（见 controller `tryPlace`）
