import {
  CanvasTexture,
  NearestFilter,
  SRGBColorSpace,
  Texture,
  type Texture as ThreeTexture,
} from "three";

import andesiteUrl from "../assets/single/blocks/andesite.png";
import bedrockUrl from "../assets/single/blocks/bedrock.png";
import diamondOreUrl from "../assets/single/blocks/diamond_ore.png";
import dirtUrl from "../assets/single/blocks/dirt.png";
import extractionUrl from "../assets/single/blocks/extraction.png";
import goldOreUrl from "../assets/single/blocks/gold_ore.png";
import grassSideOverlayUrl from "../assets/single/blocks/grass_side_overlay.png";
import grassSideUrl from "../assets/single/blocks/grass_side.png";
import grassTopUrl from "../assets/single/blocks/grass_top.png";
import oakLogUrl from "../assets/single/blocks/oak_log.png";
import oakPlanksUrl from "../assets/single/blocks/oak_planks.png";
import sandUrl from "../assets/single/blocks/sand.png";
import smoothStoneUrl from "../assets/single/blocks/smooth_stone.png";
import stoneUrl from "../assets/single/blocks/stone.png";
import { LOCAL_TEXTURE_GROUPS } from "./blocks";
import type { LocalBiomeId } from "./map-style";

/**
 * Lab/MC 草顶、草侧 overlay 是「灰阶遮罩」，运行时必须乘群系草绿，
 * 否则草地/树叶会呈灰白石色（用户截图问题根因）。
 */
const GRASS_TINT_BY_BIOME: Partial<
  Record<LocalBiomeId, readonly [number, number, number]>
> = {
  // 近似 MC plains / forest 草绿
  meadow: [0.57, 0.75, 0.28],
  spring: [0.5, 0.82, 0.32],
  rainforest: [0.28, 0.62, 0.22],
};

/**
 * 单机方块贴图：按生物群系映射不同地表材质。
 * 矿石/结构共用；草/土在荒漠=沙、雪原=雪面。
 */
export async function createLocalTextureSources(
  biome: LocalBiomeId = "meadow",
): Promise<Array<{ groupName: string; source: ThreeTexture }>> {
  const surface = surfaceUrlsForBiome(biome);
  const grassTint = GRASS_TINT_BY_BIOME[biome] ?? null;
  // 春天/雨林/草地用原木纹理做树干，其余保留木板（废墟梁感）
  const timberUrl =
    biome === "spring" || biome === "rainforest" || biome === "meadow"
      ? oakLogUrl
      : oakPlanksUrl;

  const grassTopSource =
    grassTint !== null
      ? loadTinted(LOCAL_TEXTURE_GROUPS.grassTop, surface.grassTop, grassTint)
      : load(LOCAL_TEXTURE_GROUPS.grassTop, surface.grassTop);
  const grassSideSource =
    grassTint !== null
      ? loadGrassSideComposite(
          LOCAL_TEXTURE_GROUPS.grassSide,
          surface.dirt,
          grassSideOverlayUrl,
          grassTint,
        )
      : load(LOCAL_TEXTURE_GROUPS.grassSide, surface.grassSide);
  const leavesSource =
    grassTint !== null
      ? loadTinted(LOCAL_TEXTURE_GROUPS.leaves, surface.leaves, grassTint)
      : load(LOCAL_TEXTURE_GROUPS.leaves, surface.leaves);

  return Promise.all([
    load(LOCAL_TEXTURE_GROUPS.quarryStone, surface.stone),
    load(LOCAL_TEXTURE_GROUPS.paleStone, surface.pale),
    load(LOCAL_TEXTURE_GROUPS.weatheredTimber, timberUrl),
    load(LOCAL_TEXTURE_GROUPS.bedrock, bedrockUrl),
    load(LOCAL_TEXTURE_GROUPS.dirt, surface.dirt),
    grassTopSource,
    grassSideSource,
    load(LOCAL_TEXTURE_GROUPS.grassBottom, surface.dirt),
    load(LOCAL_TEXTURE_GROUPS.gold, goldOreUrl),
    load(LOCAL_TEXTURE_GROUPS.diamond, diamondOreUrl),
    load(LOCAL_TEXTURE_GROUPS.extractionMarker, extractionUrl),
    leavesSource,
  ]);
}

/** 测试或离线降级：同步程序化占位（不依赖 PNG 加载）。 */
export function createProceduralTextureSourcesFallback(
  biome: LocalBiomeId = "meadow",
): Array<{
  groupName: string;
  source: CanvasTexture;
}> {
  const colors = proceduralColorsForBiome(biome);
  return [
    procedural(LOCAL_TEXTURE_GROUPS.quarryStone, colors.stone),
    procedural(LOCAL_TEXTURE_GROUPS.paleStone, colors.pale),
    procedural(LOCAL_TEXTURE_GROUPS.weatheredTimber, "#7a4f31"),
    procedural(LOCAL_TEXTURE_GROUPS.bedrock, "#2c3234"),
    procedural(LOCAL_TEXTURE_GROUPS.dirt, colors.dirt),
    procedural(LOCAL_TEXTURE_GROUPS.grassTop, colors.grassTop),
    procedural(LOCAL_TEXTURE_GROUPS.grassSide, colors.grassSide),
    procedural(LOCAL_TEXTURE_GROUPS.grassBottom, colors.dirt),
    procedural(LOCAL_TEXTURE_GROUPS.gold, "#c9a227"),
    procedural(LOCAL_TEXTURE_GROUPS.diamond, "#35d0cf"),
    procedural(LOCAL_TEXTURE_GROUPS.extractionMarker, "#3fad84"),
    procedural(LOCAL_TEXTURE_GROUPS.leaves, colors.leaves),
  ];
}

function surfaceUrlsForBiome(biome: LocalBiomeId): {
  stone: string;
  pale: string;
  dirt: string;
  grassTop: string;
  grassSide: string;
  leaves: string;
} {
  if (biome === "desert") {
    return {
      stone: stoneUrl,
      pale: andesiteUrl,
      dirt: sandUrl,
      grassTop: sandUrl,
      grassSide: sandUrl,
      leaves: grassTopUrl,
    };
  }
  if (biome === "snow") {
    return {
      stone: stoneUrl,
      pale: smoothStoneUrl,
      dirt: smoothStoneUrl,
      grassTop: smoothStoneUrl,
      grassSide: andesiteUrl,
      leaves: smoothStoneUrl,
    };
  }
  if (biome === "wasteland") {
    return {
      stone: stoneUrl,
      pale: andesiteUrl,
      dirt: dirtUrl,
      grassTop: dirtUrl,
      grassSide: dirtUrl,
      leaves: andesiteUrl,
    };
  }
  if (biome === "rainforest") {
    return {
      stone: stoneUrl,
      pale: andesiteUrl,
      dirt: dirtUrl,
      grassTop: grassTopUrl,
      grassSide: grassSideUrl,
      leaves: grassTopUrl,
    };
  }
  // spring / meadow：鲜绿草面 + 同系树叶
  return {
    stone: stoneUrl,
    pale: andesiteUrl,
    dirt: dirtUrl,
    grassTop: grassTopUrl,
    grassSide: grassSideUrl,
    leaves: grassTopUrl,
  };
}

function proceduralColorsForBiome(biome: LocalBiomeId): {
  stone: string;
  pale: string;
  dirt: string;
  grassTop: string;
  grassSide: string;
  leaves: string;
} {
  if (biome === "desert") {
    return {
      stone: "#8a7a60",
      pale: "#c2b48a",
      dirt: "#d8c08a",
      grassTop: "#e0c898",
      grassSide: "#cbb07a",
      leaves: "#8a9a40",
    };
  }
  if (biome === "snow") {
    return {
      stone: "#6a7078",
      pale: "#d8dee6",
      dirt: "#c8d0d8",
      grassTop: "#f2f6fa",
      grassSide: "#c5cdd6",
      leaves: "#6a8a70",
    };
  }
  if (biome === "wasteland") {
    return {
      stone: "#5a5048",
      pale: "#6e6054",
      dirt: "#5c4838",
      grassTop: "#4a3c30",
      grassSide: "#4a3c30",
      leaves: "#5a5038",
    };
  }
  if (biome === "rainforest") {
    return {
      stone: "#5a6050",
      pale: "#7a8068",
      dirt: "#5a3c28",
      grassTop: "#1f6b32",
      grassSide: "#2a5a28",
      leaves: "#1a7030",
    };
  }
  if (biome === "spring") {
    return {
      stone: "#7a7060",
      pale: "#a8a090",
      dirt: "#7d543d",
      grassTop: "#5aaa40",
      grassSide: "#5a7a38",
      leaves: "#4cba48",
    };
  }
  return {
    stone: "#7a7060",
    pale: "#a8a090",
    dirt: "#7d543d",
    grassTop: "#4f8a3d",
    grassSide: "#5a6b3a",
    leaves: "#3d8c3a",
  };
}

async function load(
  groupName: string,
  url: string,
): Promise<{ groupName: string; source: Texture }> {
  const image = await loadImage(url);
  return textureFromImage(groupName, image);
}

/** 灰阶草遮罩 × 群系色 → 真正的绿色草/叶 */
async function loadTinted(
  groupName: string,
  url: string,
  tint: readonly [number, number, number],
): Promise<{ groupName: string; source: Texture }> {
  const image = await loadImage(url);
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth || image.width || 16;
  canvas.height = image.naturalHeight || image.height || 16;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("无法创建染色画布");
  ctx.drawImage(image, 0, 0);
  const data = ctx.getImageData(0, 0, canvas.width, canvas.height);
  const [tr, tg, tb] = tint;
  for (let i = 0; i < data.data.length; i += 4) {
    if (data.data[i + 3] === 0) continue;
    // 乘算：保留遮罩明暗，染上草绿
    data.data[i] = Math.min(255, Math.round(data.data[i] * tr));
    data.data[i + 1] = Math.min(255, Math.round(data.data[i + 1] * tg));
    data.data[i + 2] = Math.min(255, Math.round(data.data[i + 2] * tb));
  }
  ctx.putImageData(data, 0, 0);
  return textureFromCanvas(groupName, canvas);
}

/**
 * 草侧面：dirt 底 + 染色 overlay（与 Lab/MC 合成一致，避免整侧变绿泥）。
 */
async function loadGrassSideComposite(
  groupName: string,
  dirtUrlSrc: string,
  overlayUrl: string,
  tint: readonly [number, number, number],
): Promise<{ groupName: string; source: Texture }> {
  const [dirtImg, overlayImg] = await Promise.all([
    loadImage(dirtUrlSrc),
    loadImage(overlayUrl),
  ]);
  const size = 16;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("无法创建草侧面画布");
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(dirtImg, 0, 0, size, size);

  // overlay 先画到临时画布再染色，再 alpha 叠到 dirt 上
  const overlayCanvas = document.createElement("canvas");
  overlayCanvas.width = size;
  overlayCanvas.height = size;
  const octx = overlayCanvas.getContext("2d");
  if (octx === null) throw new Error("无法创建草侧 overlay 画布");
  octx.imageSmoothingEnabled = false;
  octx.drawImage(overlayImg, 0, 0, size, size);
  const data = octx.getImageData(0, 0, size, size);
  const [tr, tg, tb] = tint;
  for (let i = 0; i < data.data.length; i += 4) {
    if (data.data[i + 3] === 0) continue;
    data.data[i] = Math.min(255, Math.round(data.data[i] * tr));
    data.data[i + 1] = Math.min(255, Math.round(data.data[i + 1] * tg));
    data.data[i + 2] = Math.min(255, Math.round(data.data[i + 2] * tb));
  }
  octx.putImageData(data, 0, 0);
  ctx.drawImage(overlayCanvas, 0, 0);
  return textureFromCanvas(groupName, canvas);
}

function textureFromImage(
  groupName: string,
  image: HTMLImageElement,
): { groupName: string; source: Texture } {
  const source = new Texture(image);
  source.colorSpace = SRGBColorSpace;
  source.magFilter = NearestFilter;
  source.minFilter = NearestFilter;
  source.generateMipmaps = false;
  source.needsUpdate = true;
  return { groupName, source };
}

function textureFromCanvas(
  groupName: string,
  canvas: HTMLCanvasElement,
): { groupName: string; source: CanvasTexture } {
  const source = new CanvasTexture(canvas);
  source.colorSpace = SRGBColorSpace;
  source.magFilter = NearestFilter;
  source.minFilter = NearestFilter;
  source.generateMipmaps = false;
  source.needsUpdate = true;
  return { groupName, source };
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`无法加载单机贴图 ${url}`));
    image.src = url;
  });
}

function procedural(
  groupName: string,
  color: string,
): { groupName: string; source: CanvasTexture } {
  const canvas = document.createElement("canvas");
  canvas.width = 16;
  canvas.height = 16;
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("无法创建降级贴图画布");
  context.fillStyle = color;
  context.fillRect(0, 0, 16, 16);
  return textureFromCanvas(groupName, canvas);
}
