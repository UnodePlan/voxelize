# Single-player block textures

## Source

Extracted from `https://create.town/lab` Next.js static media:

`/_next/static/media/<name>.<hash>.png`

PRD for task `07-14-extraction-single-player-vertical-slice` allows porting Lab art assets and localizing them into the repo.

## Mapping

| Local file | Lab media name | Use |
|---|---|---|
| `dirt.png` | `dirt` | dirt / grass bottom |
| `grass_top.png` | `grass_block_top` | grass top |
| `grass_side_overlay.png` | `grass_block_side_overlay` | composite input |
| `grass_side.png` | dirt + overlay composite | grass sides |
| `grass.png` | `grass` | spare |
| `stone.png` | `stone` | quarry stone |
| `andesite.png` | `andersite_block` / `andesite` | pale stone |
| `cobblestone.png` | `cobblestone` | spare |
| `smooth_stone.png` | `smooth_stone` | spare |
| `oak_planks.png` | `oak_planks` | weathered timber |
| `oak_log.png` / `oak_log_top.png` | oak log | spare |
| `deepslate.png` | `deepslate` | bedrock base |
| `bedrock.png` | copy of deepslate | bedrock |
| `obsidian.png` | `obsidian` | spare |
| `glowstone.png` | `glowstone` | spare / extraction base |
| `gold_ore.png` | Lab `stone` + gold flecks | gold resource |
| `diamond_ore.png` | Lab `stone` + cyan flecks | diamond resource |
| `extraction.png` | glowstone recolored mint | extraction marker |
| `oak_log.png` | oak log | spring/meadow/rainforest timber |

## Notes

- All runtime-facing block textures are 16×16, nearest-neighbor friendly.
- Do not fetch create.town at runtime; assets are vendored under this directory.
