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

| Hotbar slot (1-based keys) | Held | Arm API |
|---|---|---|
| 1 | empty hand | `setArmObject(undefined)` → CanvasBox arm |
| 2 | iron pickaxe | plane sprite + `customType: "item"` |
| 3 | iron sword | same as pickaxe, different texture |
| 4+ | resources only | empty hand in FP |

Inventory rule: slots 0–2 never store resources (`LOCAL_TOOL_HOTBAR_SLOTS` in `state.ts`).  
Runtime:
- 第一人称：`LocalViewmodel`（`viewmodel.ts`）
- 第三人称假人：`createMcHeldItemMesh`（`mc-held-item.ts`）挂到 `McBipedMannequin` 右臂
