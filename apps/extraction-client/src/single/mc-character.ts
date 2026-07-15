/**
 * 近似原版 MC 比例的方块人 + 标准 64×64 皮肤 UV。
 * 皮肤为仓库内自绘 PNG（非 Mojang jar 解包），UV 遵循社区 skin-spec。
 */

import { Character } from "@voxelize/core";

import defaultSkinUrl from "../assets/single/skins/steve_classic.png";

import {
  applyMcSkinToCharacter,
  loadSkinImage,
} from "./mc-skin";

/** MC 1 像素 = 1/16 方块 */
export const MC_PIXEL = 1 / 16;

/** 回退纯色（皮肤加载前/失败） */
const SKIN = "#c68c6c";
const SHIRT = "#00a8a8";
const PANTS = "#3d44a8";

export interface McCharacterOptions {
  username?: string;
  /** 整体缩放，默认 1 */
  scale?: number;
  /**
   * 皮肤 PNG URL；默认自绘 steve_classic。
   * 须为标准 64×64（或 64×32）布局。
   */
  skinUrl?: string;
}

/** 纯数据：MC 比例（便于单测） */
export function mcCharacterMetrics(scale = 1) {
  const u = MC_PIXEL * scale;
  const head = 8 * u;
  const bodyH = 12 * u;
  const bodyW = 8 * u;
  const bodyD = 4 * u;
  const limbW = 4 * u;
  const limbH = 12 * u;
  return {
    unit: u,
    head: { width: head, height: head, depth: head },
    body: { width: bodyW, height: bodyH, depth: bodyD },
    limb: { width: limbW, height: limbH, depth: limbW },
    totalHeight: limbH + bodyH + head,
    eyeHeight: limbH + bodyH + head / 2,
  };
}

/**
 * 同步创建带 MC 比例的 Character（纯色占位）。
 * 皮肤请再调 `paintMcCharacterSkin` / `createMcStyleCharacter`。
 */
export function createMcCharacterMesh(
  options: McCharacterOptions = {},
): Character {
  const m = mcCharacterMetrics(options.scale ?? 1);
  const { head, body, limb } = m;
  // 臂内收：Character 默认臂心在身侧再外偏半臂宽，负 shoulderGap 贴紧
  const armInset = limb.width / 2;

  // 双层 + 更高 segments：128 皮肤的细节能落到更多像素格上
  const overlayGap = m.unit * 0.22;
  const headSeg = 16; // 脸 16×16
  const bodyWSeg = 16;
  const bodyHSeg = 24;
  const bodyDSeg = 8;
  const limbWSeg = 8;
  const limbHSeg = 24;

  return new Character({
    idleArmSwing: 0.04,
    walkingSpeed: 1.25,
    nameTagOptions: {
      fontSize: 0.18,
      yOffset: 0.28,
    },
    head: {
      width: head.width,
      height: head.height,
      depth: head.depth,
      widthSegments: headSeg,
      heightSegments: headSeg,
      depthSegments: headSeg,
      neckGap: 0,
      layers: 2,
      gap: overlayGap,
      transparent: true,
      color: SKIN,
      faceColor: SKIN,
    },
    body: {
      width: body.width,
      height: body.height,
      depth: body.depth,
      widthSegments: bodyWSeg,
      heightSegments: bodyHSeg,
      depthSegments: bodyDSeg,
      layers: 2,
      gap: overlayGap * 0.7,
      transparent: true,
      color: SHIRT,
    },
    arms: {
      width: limb.width,
      height: limb.height,
      depth: limb.depth,
      widthSegments: limbWSeg,
      heightSegments: limbHSeg,
      depthSegments: limbWSeg,
      layers: 2,
      gap: overlayGap * 0.7,
      transparent: true,
      shoulderGap: -armInset,
      shoulderDrop: 0,
      color: SKIN,
    },
    legs: {
      width: limb.width,
      height: limb.height,
      depth: limb.depth,
      widthSegments: limbWSeg,
      heightSegments: limbHSeg,
      depthSegments: limbWSeg,
      layers: 2,
      gap: overlayGap * 0.7,
      transparent: true,
      betweenLegsGap: 0,
      color: PANTS,
    },
  });
}

/** 异步加载皮肤并贴到已有 Character */
export async function paintMcCharacterSkin(
  character: Character,
  skinUrl: string = defaultSkinUrl,
): Promise<void> {
  const image = await loadSkinImage(skinUrl);
  applyMcSkinToCharacter(character, image);
}

/**
 * 创建 MC 比例方块人并贴上 64×64 皮肤（推荐入口）。
 */
export async function createMcStyleCharacter(
  options: McCharacterOptions = {},
): Promise<Character> {
  const character = createMcCharacterMesh(options);
  character.username = options.username ?? "";
  try {
    await paintMcCharacterSkin(character, options.skinUrl ?? defaultSkinUrl);
  } catch {
    // 加载失败保留纯色占位，避免整局挂掉
  }
  return character;
}
