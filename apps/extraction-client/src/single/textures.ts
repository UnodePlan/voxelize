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
import grassSideUrl from "../assets/single/blocks/grass_side.png";
import grassTopUrl from "../assets/single/blocks/grass_top.png";
import oakPlanksUrl from "../assets/single/blocks/oak_planks.png";
import stoneUrl from "../assets/single/blocks/stone.png";

import { LOCAL_TEXTURE_GROUPS } from "./blocks";

/**
 * 单机方块贴图：搬运自 create.town/lab 静态 media（见 assets/single/blocks/SOURCES.md）。
 * 矿石在 Lab 石材基底上叠加矿脉色点；草侧面为 dirt + overlay 合成。
 */
export async function createLocalTextureSources(): Promise<
  Array<{ groupName: string; source: ThreeTexture }>
> {
  return Promise.all([
    load(LOCAL_TEXTURE_GROUPS.quarryStone, stoneUrl),
    load(LOCAL_TEXTURE_GROUPS.paleStone, andesiteUrl),
    load(LOCAL_TEXTURE_GROUPS.weatheredTimber, oakPlanksUrl),
    load(LOCAL_TEXTURE_GROUPS.bedrock, bedrockUrl),
    load(LOCAL_TEXTURE_GROUPS.dirt, dirtUrl),
    load(LOCAL_TEXTURE_GROUPS.grassTop, grassTopUrl),
    load(LOCAL_TEXTURE_GROUPS.grassSide, grassSideUrl),
    load(LOCAL_TEXTURE_GROUPS.grassBottom, dirtUrl),
    load(LOCAL_TEXTURE_GROUPS.gold, goldOreUrl),
    load(LOCAL_TEXTURE_GROUPS.diamond, diamondOreUrl),
    load(LOCAL_TEXTURE_GROUPS.extractionMarker, extractionUrl),
  ]);
}

/** 测试或离线降级：同步程序化占位（不依赖 PNG 加载）。 */
export function createProceduralTextureSourcesFallback(): Array<{
  groupName: string;
  source: CanvasTexture;
}> {
  return [
    procedural(LOCAL_TEXTURE_GROUPS.quarryStone, "#7a7060"),
    procedural(LOCAL_TEXTURE_GROUPS.paleStone, "#a8a090"),
    procedural(LOCAL_TEXTURE_GROUPS.weatheredTimber, "#7a4f31"),
    procedural(LOCAL_TEXTURE_GROUPS.bedrock, "#2c3234"),
    procedural(LOCAL_TEXTURE_GROUPS.dirt, "#7d543d"),
    procedural(LOCAL_TEXTURE_GROUPS.grassTop, "#4f8a3d"),
    procedural(LOCAL_TEXTURE_GROUPS.grassSide, "#5a6b3a"),
    procedural(LOCAL_TEXTURE_GROUPS.grassBottom, "#7d543d"),
    procedural(LOCAL_TEXTURE_GROUPS.gold, "#c9a227"),
    procedural(LOCAL_TEXTURE_GROUPS.diamond, "#35d0cf"),
    procedural(LOCAL_TEXTURE_GROUPS.extractionMarker, "#3fad84"),
  ];
}

async function load(
  groupName: string,
  url: string,
): Promise<{ groupName: string; source: Texture }> {
  const image = await loadImage(url);
  const source = new Texture(image);
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
  const source = new CanvasTexture(canvas);
  source.colorSpace = SRGBColorSpace;
  source.magFilter = NearestFilter;
  source.minFilter = NearestFilter;
  source.generateMipmaps = false;
  source.needsUpdate = true;
  return { groupName, source };
}
