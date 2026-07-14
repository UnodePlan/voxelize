import { LOCAL_BLOCK_IDS } from "./blocks";

/**
 * 玩法点只标 x/z；真实 y 由程序化高度图决定（见 map.ts）。
 * 出生偏南，撤离在中心，矿点散落附近。
 */

export const LOCAL_SPAWN_XZ = [0, 10] as const;
export const LOCAL_EXTRACTION_XZ = [0, 0] as const;

/** 出生 → 资源带 → 撤离区的 xz 折线 */
export const LOCAL_ROUTE_XZ: ReadonlyArray<readonly [number, number]> = [
  [0, 10],
  [0, 4],
  [8, 4],
  [12, 0],
  [8, -6],
  [0, -6],
  [0, 0],
];

/** 地表可采矿：x, z, blockId */
export const LOCAL_RESOURCE_SPOTS: ReadonlyArray<
  readonly [number, number, number]
> = [
  [6, 6, LOCAL_BLOCK_IDS.dirt],
  [7, 6, LOCAL_BLOCK_IDS.dirt],
  [6, 7, LOCAL_BLOCK_IDS.dirt],
  [8, 5, LOCAL_BLOCK_IDS.dirt],
  [5, 8, LOCAL_BLOCK_IDS.dirt],
  [14, 2, LOCAL_BLOCK_IDS.gold],
  [15, 2, LOCAL_BLOCK_IDS.gold],
  [14, 3, LOCAL_BLOCK_IDS.gold],
  [16, 1, LOCAL_BLOCK_IDS.gold],
  [13, 1, LOCAL_BLOCK_IDS.gold],
  [-8, -4, LOCAL_BLOCK_IDS.diamond],
  [-9, -4, LOCAL_BLOCK_IDS.diamond],
  [-8, -5, LOCAL_BLOCK_IDS.diamond],
  [-7, -3, LOCAL_BLOCK_IDS.diamond],
  [-10, -5, LOCAL_BLOCK_IDS.diamond],
];
