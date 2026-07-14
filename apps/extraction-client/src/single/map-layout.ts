/**
 * 玩法锚点只标 x/z；真实 y 由程序化高度图决定（见 map.ts）。
 * 出生偏南，撤离在中心；矿点每局按种子随机散布。
 */

export const LOCAL_SPAWN_XZ = [0, 10] as const;
export const LOCAL_EXTRACTION_XZ = [0, 0] as const;

/** 出生 → 资源带方向 → 撤离区的 xz 折线（地表路线，每局高度随地形变） */
export const LOCAL_ROUTE_XZ: ReadonlyArray<readonly [number, number]> = [
  [0, 10],
  [0, 4],
  [8, 4],
  [12, 0],
  [8, -6],
  [0, -6],
  [0, 0],
];

/**
 * 每局地表可采矿配额。
 * 泥土常见、黄金中等、钻石稀少偏远。
 */
export const LOCAL_RESOURCE_QUOTAS = {
  dirt: 10,
  gold: 7,
  diamond: 5,
} as const;

/** 出生营 / 撤离广场周围不刷矿，避免开局踩矿或挡撤离 */
export const LOCAL_RESOURCE_SPAWN_CLEAR_RADIUS = 5;
export const LOCAL_RESOURCE_EXTRACT_CLEAR_RADIUS = 4;
