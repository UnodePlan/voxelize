import type { ResourceCounts } from "../api/models";

import { LOCAL_FACE_TEMPLATES } from "./block-faces";
import {
  miningDurationMs,
  type LocalPreferredTool,
  type VanillaMiningInput,
} from "./mining";
import type { LocalResourceKey } from "./state";

export const LOCAL_BLOCK_IDS = {
  air: 0,
  quarryStone: 1,
  paleStone: 2,
  weatheredTimber: 3,
  bedrock: 4,
  extractionMarker: 5,
  grass: 6,
  dirt: 1001,
  gold: 1002,
  diamond: 1003,
  leaves: 1004,
} as const;

export const LOCAL_TEXTURE_GROUPS = {
  quarryStone: "single-quarry-stone",
  paleStone: "single-pale-stone",
  weatheredTimber: "single-weathered-timber",
  bedrock: "single-bedrock",
  dirt: "single-dirt",
  grassTop: "single-grass-top",
  grassSide: "single-grass-side",
  grassBottom: "single-grass-bottom",
  gold: "single-gold",
  diamond: "single-diamond",
  extractionMarker: "single-extraction-marker",
  leaves: "single-leaves",
} as const;

/**
 * 可破坏方块的挖掘档案（hardness 取自 MC 1.21.4 / minecraft-data）。
 * 未列出的 id（air/基岩/信标）不可挖。
 */
export interface LocalBlockMiningProfile extends VanillaMiningInput {
  displayName: string;
  /** 正确收获时进背包的资源；结构块为 null */
  drop: LocalResourceKey | null;
  preferredTool: LocalPreferredTool;
}

/**
 * 单机挖掘表：严格原版 hardness + requiresCorrectToolForDrops。
 * 耗时见 mining.ts 的 destroySpeed / 30|100 公式。
 */
export const LOCAL_BLOCK_MINING: Readonly<
  Record<number, LocalBlockMiningProfile>
> = {
  // grass_block hardness 0.6，mineable/shovel，手可掉落
  [LOCAL_BLOCK_IDS.grass]: {
    displayName: "草地",
    hardness: 0.6,
    preferredTool: "shovel",
    requiresCorrectToolForDrops: false,
    drop: "dirt",
  },
  // dirt 0.5
  [LOCAL_BLOCK_IDS.dirt]: {
    displayName: "泥土",
    hardness: 0.5,
    preferredTool: "shovel",
    requiresCorrectToolForDrops: false,
    drop: "dirt",
  },
  // oak_planks 2.0，mineable/axe，不要求正确工具掉落
  [LOCAL_BLOCK_IDS.weatheredTimber]: {
    displayName: "旧木梁",
    hardness: 2.0,
    preferredTool: "axe",
    requiresCorrectToolForDrops: false,
    drop: null,
  },
  // stone 1.5，需镐才掉落（结构块无掉落）
  [LOCAL_BLOCK_IDS.paleStone]: {
    displayName: "风化石台",
    hardness: 1.5,
    preferredTool: "pickaxe",
    requiresCorrectToolForDrops: true,
    drop: null,
  },
  // cobblestone 2.0 作为采石岩壁
  [LOCAL_BLOCK_IDS.quarryStone]: {
    displayName: "采石场岩壁",
    hardness: 2.0,
    preferredTool: "pickaxe",
    requiresCorrectToolForDrops: true,
    drop: null,
  },
  // gold_ore 3.0，需铁镐+
  [LOCAL_BLOCK_IDS.gold]: {
    displayName: "黄金矿",
    hardness: 3.0,
    preferredTool: "pickaxe",
    requiresCorrectToolForDrops: true,
    drop: "gold",
  },
  // diamond_ore 3.0，需铁镐+
  [LOCAL_BLOCK_IDS.diamond]: {
    displayName: "钻石矿",
    hardness: 3.0,
    preferredTool: "pickaxe",
    requiresCorrectToolForDrops: true,
    drop: "diamond",
  },
  // oak_leaves 0.2，手可破、无掉落（简化）
  [LOCAL_BLOCK_IDS.leaves]: {
    displayName: "树叶",
    hardness: 0.2,
    preferredTool: "axe",
    requiresCorrectToolForDrops: false,
    drop: null,
  },
};

/** 兼容旧引用：仅含「存在掉落定义」的资源方块；时长=空手原版 ms */
export const LOCAL_MINEABLE_BLOCKS: Readonly<
  Record<
    number,
    {
      displayName: string;
      miningDurationMs: number;
      resource: LocalResourceKey;
    }
  >
> = Object.fromEntries(
  Object.entries(LOCAL_BLOCK_MINING)
    .filter(([, profile]) => profile.drop !== null)
    .map(([id, profile]) => [
      Number(id),
      {
        displayName: profile.displayName,
        miningDurationMs: miningDurationMs(profile, "empty"),
        resource: profile.drop as LocalResourceKey,
      },
    ]),
) as Readonly<
  Record<
    number,
    {
      displayName: string;
      miningDurationMs: number;
      resource: LocalResourceKey;
    }
  >
>;

export function getBlockMiningProfile(
  blockId: number,
): LocalBlockMiningProfile | null {
  return LOCAL_BLOCK_MINING[blockId] ?? null;
}

export function isBlockMineable(blockId: number): boolean {
  const profile = getBlockMiningProfile(blockId);
  return profile !== null && profile.hardness >= 0;
}

export const LOCAL_BLOCK_DISPLAY_NAMES: Readonly<Record<number, string>> = {
  [LOCAL_BLOCK_IDS.quarryStone]: "采石场岩壁",
  [LOCAL_BLOCK_IDS.paleStone]: "风化石台",
  [LOCAL_BLOCK_IDS.weatheredTimber]: "旧木梁",
  [LOCAL_BLOCK_IDS.bedrock]: "基岩",
  [LOCAL_BLOCK_IDS.extractionMarker]: "撤离信标",
  [LOCAL_BLOCK_IDS.grass]: "草地",
  [LOCAL_BLOCK_IDS.dirt]: "泥土",
  [LOCAL_BLOCK_IDS.gold]: "黄金矿",
  [LOCAL_BLOCK_IDS.diamond]: "钻石矿",
  [LOCAL_BLOCK_IDS.leaves]: "树叶",
};

/** 破坏碎片/掉落近似色（无贴图时的 fallback） */
export const LOCAL_BLOCK_DEBRIS_COLORS: Readonly<Record<number, string>> = {
  [LOCAL_BLOCK_IDS.quarryStone]: "#8a8a8a",
  [LOCAL_BLOCK_IDS.paleStone]: "#b5b5a8",
  [LOCAL_BLOCK_IDS.weatheredTimber]: "#9a7348",
  [LOCAL_BLOCK_IDS.grass]: "#5d8a3a",
  [LOCAL_BLOCK_IDS.dirt]: "#79523d",
  [LOCAL_BLOCK_IDS.gold]: "#c9a227",
  [LOCAL_BLOCK_IDS.diamond]: "#3dd2cc",
  [LOCAL_BLOCK_IDS.leaves]: "#3d8c3a",
};

export interface LocalBlockFace {
  corners: Array<{ pos: [number, number, number]; uv: [number, number] }>;
  dir: [number, number, number];
  independent: boolean;
  isolated: boolean;
  name: string;
  range: { startU: number; endU: number; startV: number; endV: number };
  textureGroup: string | null;
}

export interface LocalSerializedBlock {
  aabbs: Array<{
    minX: number;
    minY: number;
    minZ: number;
    maxX: number;
    maxY: number;
    maxZ: number;
  }>;
  blueLightLevel: number;
  defaultEntityJson: string | null;
  dynamicPatterns: null;
  faces: LocalBlockFace[];
  fluidFlowForce: number;
  greenLightLevel: number;
  groundFrictionMultiplier: number;
  id: number;
  isActive: boolean;
  isClimbable: boolean;
  isDynamic: boolean;
  isEmpty: boolean;
  isEntity: boolean;
  isFluid: boolean;
  isLight: boolean;
  isOpaque: boolean;
  isPassable: boolean;
  isPlant: boolean;
  isSeeThrough: boolean;
  isTransparent: [boolean, boolean, boolean, boolean, boolean, boolean];
  isWaterlogged: boolean;
  lightReduce: boolean;
  name: string;
  occludesFluid: boolean;
  redLightLevel: number;
  rotatable: boolean;
  transparentStandalone: boolean;
  yRotatable: boolean;
  yRotatableSegments: "All";
}

export function createLocalBlocks(): Record<string, LocalSerializedBlock> {
  return {
    Air: makeAirBlock(),
    "Quarry Stone": makeSolidBlock(
      LOCAL_BLOCK_IDS.quarryStone,
      "Quarry Stone",
      LOCAL_TEXTURE_GROUPS.quarryStone,
      0,
    ),
    "Pale Stone": makeSolidBlock(
      LOCAL_BLOCK_IDS.paleStone,
      "Pale Stone",
      LOCAL_TEXTURE_GROUPS.paleStone,
      1,
    ),
    "Weathered Timber": makeSolidBlock(
      LOCAL_BLOCK_IDS.weatheredTimber,
      "Weathered Timber",
      LOCAL_TEXTURE_GROUPS.weatheredTimber,
      2,
    ),
    Bedrock: makeSolidBlock(
      LOCAL_BLOCK_IDS.bedrock,
      "Bedrock",
      LOCAL_TEXTURE_GROUPS.bedrock,
      3,
    ),
    Grass: makeMultiFaceBlock(LOCAL_BLOCK_IDS.grass, "Grass", {
      px: { group: LOCAL_TEXTURE_GROUPS.grassSide, index: 4 },
      nx: { group: LOCAL_TEXTURE_GROUPS.grassSide, index: 4 },
      pz: { group: LOCAL_TEXTURE_GROUPS.grassSide, index: 4 },
      nz: { group: LOCAL_TEXTURE_GROUPS.grassSide, index: 4 },
      py: { group: LOCAL_TEXTURE_GROUPS.grassTop, index: 5 },
      ny: { group: LOCAL_TEXTURE_GROUPS.grassBottom, index: 6 },
    }),
    Dirt: makeSolidBlock(
      LOCAL_BLOCK_IDS.dirt,
      "Dirt",
      LOCAL_TEXTURE_GROUPS.dirt,
      7,
    ),
    Gold: makeSolidBlock(
      LOCAL_BLOCK_IDS.gold,
      "Gold",
      LOCAL_TEXTURE_GROUPS.gold,
      8,
    ),
    Diamond: makeSolidBlock(
      LOCAL_BLOCK_IDS.diamond,
      "Diamond",
      LOCAL_TEXTURE_GROUPS.diamond,
      9,
    ),
    "Extraction Marker": makeSolidBlock(
      LOCAL_BLOCK_IDS.extractionMarker,
      "Extraction Marker",
      LOCAL_TEXTURE_GROUPS.extractionMarker,
      10,
      {
        aabbs: [],
        blueLightLevel: 10,
        greenLightLevel: 15,
        isOpaque: false,
        isPassable: true,
        isSeeThrough: true,
        isTransparent: [true, true, true, true, true, true],
        redLightLevel: 4,
        transparentStandalone: true,
      },
    ),
    Leaves: makeSolidBlock(
      LOCAL_BLOCK_IDS.leaves,
      "Leaves",
      LOCAL_TEXTURE_GROUPS.leaves,
      11,
      {
        // 半透树冠：可挡视线但不完全遮光
        isOpaque: false,
        isSeeThrough: true,
        isTransparent: [true, true, true, true, true, true],
        lightReduce: true,
        transparentStandalone: true,
      },
    ),
  };
}

export function resourceCountsFromIds(ids: readonly number[]): ResourceCounts {
  const counts: ResourceCounts = { dirt: 0, gold: 0, diamond: 0 };
  for (const id of ids) {
    const drop = LOCAL_BLOCK_MINING[id]?.drop;
    if (drop !== null && drop !== undefined) counts[drop] += 1;
  }
  return counts;
}

function makeAirBlock(): LocalSerializedBlock {
  return makeBlock(LOCAL_BLOCK_IDS.air, "Air", [], {
    aabbs: [],
    isEmpty: true,
    isOpaque: false,
    isPassable: true,
    isTransparent: [true, true, true, true, true, true],
  });
}

function makeSolidBlock(
  id: number,
  name: string,
  textureGroup: string,
  textureIndex: number,
  options: Partial<LocalSerializedBlock> = {},
): LocalSerializedBlock {
  return makeBlock(id, name, makeFaces(textureGroup, textureIndex), options);
}

function makeMultiFaceBlock(
  id: number,
  name: string,
  faceGroups: Readonly<
    Record<string, { group: string; index: number }>
  >,
  options: Partial<LocalSerializedBlock> = {},
): LocalSerializedBlock {
  const faces = LOCAL_FACE_TEMPLATES.map((face) => {
    const mapping = faceGroups[face.name] ?? faceGroups.py;
    return {
      name: face.name,
      dir: [...face.dir] as [number, number, number],
      independent: false,
      isolated: false,
      textureGroup: mapping.group,
      range: makeAtlasRange(mapping.index),
      corners: face.corners.map(([pos, uv]) => ({
        pos: [...pos] as [number, number, number],
        uv: [...uv] as [number, number],
      })),
    };
  });
  return makeBlock(id, name, faces, options);
}

function makeBlock(
  id: number,
  name: string,
  faces: LocalBlockFace[],
  options: Partial<LocalSerializedBlock>,
): LocalSerializedBlock {
  const redLightLevel = options.redLightLevel ?? 0;
  const greenLightLevel = options.greenLightLevel ?? 0;
  const blueLightLevel = options.blueLightLevel ?? 0;
  return {
    id,
    name,
    rotatable: false,
    yRotatable: false,
    yRotatableSegments: "All",
    isEmpty: false,
    isFluid: false,
    fluidFlowForce: 0,
    groundFrictionMultiplier: 1,
    isWaterlogged: false,
    isLight: redLightLevel > 0 || greenLightLevel > 0 || blueLightLevel > 0,
    isPassable: false,
    isClimbable: false,
    isOpaque: true,
    redLightLevel,
    greenLightLevel,
    blueLightLevel,
    transparentStandalone: false,
    faces,
    aabbs: [{ minX: 0, minY: 0, minZ: 0, maxX: 1, maxY: 1, maxZ: 1 }],
    isSeeThrough: false,
    occludesFluid: false,
    isPlant: false,
    isTransparent: [false, false, false, false, false, false],
    lightReduce: true,
    isEntity: false,
    defaultEntityJson: null,
    isDynamic: false,
    dynamicPatterns: null,
    isActive: false,
    ...options,
  };
}

function makeFaces(
  textureGroup: string,
  textureIndex: number,
): LocalBlockFace[] {
  const range = makeAtlasRange(textureIndex);
  return LOCAL_FACE_TEMPLATES.map((face) => ({
    name: face.name,
    dir: [...face.dir],
    independent: false,
    isolated: false,
    textureGroup,
    range: { ...range },
    corners: face.corners.map(([pos, uv]) => ({ pos: [...pos], uv: [...uv] })),
  }));
}

function makeAtlasRange(index: number) {
  // 4×4 图集格子，给草地/矿石等多纹理留出空间。
  const perSide = 4;
  const inset = 1 / (perSide * 8);
  const column = index % perSide;
  const row = Math.floor(index / perSide);
  return {
    startU: column / perSide + inset,
    endU: (column + 1) / perSide - inset,
    startV: row / perSide + inset,
    endV: (row + 1) / perSide - inset,
  };
}
