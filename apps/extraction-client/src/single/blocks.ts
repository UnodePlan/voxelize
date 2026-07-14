import type { ResourceCounts } from "../api/models";

import { LOCAL_FACE_TEMPLATES } from "./block-faces";
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
} as const;

export const LOCAL_MINEABLE_BLOCKS: Readonly<
  Record<
    number,
    {
      displayName: string;
      miningDurationMs: number;
      resource: LocalResourceKey;
    }
  >
> = {
  [LOCAL_BLOCK_IDS.dirt]: {
    displayName: "泥土",
    miningDurationMs: 500,
    resource: "dirt",
  },
  [LOCAL_BLOCK_IDS.gold]: {
    displayName: "黄金矿",
    miningDurationMs: 1_500,
    resource: "gold",
  },
  [LOCAL_BLOCK_IDS.diamond]: {
    displayName: "钻石矿",
    miningDurationMs: 3_000,
    resource: "diamond",
  },
};

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
  };
}

export function resourceCountsFromIds(ids: readonly number[]): ResourceCounts {
  const counts: ResourceCounts = { dirt: 0, gold: 0, diamond: 0 };
  for (const id of ids) {
    const resource = LOCAL_MINEABLE_BLOCKS[id]?.resource;
    if (resource !== undefined) counts[resource] += 1;
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
