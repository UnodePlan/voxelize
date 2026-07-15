/**
 * MC 皮肤 UV → Character.CanvasBox。
 * 支持 64×64 与 128×128（2× HD：区域坐标自动 × scale）。
 */

import type { Character } from "@voxelize/core";
import { NearestFilter, SRGBColorSpace, Texture } from "three";

/** 皮肤图上的矩形 [x, y, w, h]（以 64 图为基准；HD 时乘 scale） */
export type SkinRect = readonly [number, number, number, number];

/** Classic 粗臂主层（64 基准） */
export const MC_SKIN_UV = {
  head: {
    top: [8, 0, 8, 8],
    bottom: [16, 0, 8, 8],
    right: [0, 8, 8, 8],
    front: [8, 8, 8, 8],
    left: [16, 8, 8, 8],
    back: [24, 8, 8, 8],
  },
  body: {
    top: [20, 16, 8, 4],
    bottom: [28, 16, 8, 4],
    right: [16, 20, 4, 12],
    front: [20, 20, 8, 12],
    left: [28, 20, 4, 12],
    back: [32, 20, 8, 12],
  },
  rightArm: {
    top: [44, 16, 4, 4],
    bottom: [48, 16, 4, 4],
    right: [40, 20, 4, 12],
    front: [44, 20, 4, 12],
    left: [48, 20, 4, 12],
    back: [52, 20, 4, 12],
  },
  leftArm: {
    top: [36, 48, 4, 4],
    bottom: [40, 48, 4, 4],
    right: [32, 52, 4, 12],
    front: [36, 52, 4, 12],
    left: [40, 52, 4, 12],
    back: [44, 52, 4, 12],
  },
  rightLeg: {
    top: [4, 16, 4, 4],
    bottom: [8, 16, 4, 4],
    right: [0, 20, 4, 12],
    front: [4, 20, 4, 12],
    left: [8, 20, 4, 12],
    back: [12, 20, 4, 12],
  },
  leftLeg: {
    top: [20, 48, 4, 4],
    bottom: [24, 48, 4, 4],
    right: [16, 52, 4, 12],
    front: [20, 52, 4, 12],
    left: [24, 52, 4, 12],
    back: [28, 52, 4, 12],
  },
} as const satisfies Record<string, Record<string, SkinRect>>;

/** 第二层 overlay（64 基准） */
export const MC_SKIN_OVERLAY_UV = {
  head: {
    top: [40, 0, 8, 8],
    bottom: [48, 0, 8, 8],
    right: [32, 8, 8, 8],
    front: [40, 8, 8, 8],
    left: [48, 8, 8, 8],
    back: [56, 8, 8, 8],
  },
  body: {
    top: [20, 32, 8, 4],
    bottom: [28, 32, 8, 4],
    right: [16, 36, 4, 12],
    front: [20, 36, 8, 12],
    left: [28, 36, 4, 12],
    back: [32, 36, 8, 12],
  },
  rightArm: {
    top: [44, 32, 4, 4],
    bottom: [48, 32, 4, 4],
    right: [40, 36, 4, 12],
    front: [44, 36, 4, 12],
    left: [48, 36, 4, 12],
    back: [52, 36, 4, 12],
  },
  leftArm: {
    top: [52, 48, 4, 4],
    bottom: [56, 48, 4, 4],
    right: [48, 52, 4, 12],
    front: [52, 52, 4, 12],
    left: [56, 52, 4, 12],
    back: [60, 52, 4, 12],
  },
  rightLeg: {
    top: [4, 32, 4, 4],
    bottom: [8, 32, 4, 4],
    right: [0, 36, 4, 12],
    front: [4, 36, 4, 12],
    left: [8, 36, 4, 12],
    back: [12, 36, 4, 12],
  },
  leftLeg: {
    top: [4, 48, 4, 4],
    bottom: [8, 48, 4, 4],
    right: [0, 52, 4, 12],
    front: [4, 52, 4, 12],
    left: [8, 52, 4, 12],
    back: [12, 52, 4, 12],
  },
} as const satisfies Record<string, Record<string, SkinRect>>;

type FaceName = "front" | "back" | "left" | "right" | "top" | "bottom";

type Paintable = {
  paint: (
    side: FaceName,
    art: (ctx: CanvasRenderingContext2D, canvas: HTMLCanvasElement) => void,
    layer?: number,
  ) => void;
};

/** 皮肤边长 / 64 → 1 或 2（HD） */
export function skinScaleOf(atlas: HTMLCanvasElement): number {
  const w = atlas.width || 64;
  if (w >= 128) return Math.floor(w / 64);
  return 1;
}

export function scaleRect(rect: SkinRect, scale: number): SkinRect {
  if (scale === 1) return rect;
  const [x, y, w, h] = rect;
  return [x * scale, y * scale, w * scale, h * scale];
}

/**
 * 把皮肤画到 Character：layer0 主层，layer1 overlay。
 */
export function applyMcSkinToCharacter(
  character: Character,
  skin: HTMLImageElement | HTMLCanvasElement | ImageBitmap,
): void {
  const atlas = ensureCanvas(skin);
  const scale = skinScaleOf(atlas);

  paintPart(character.head, atlas, MC_SKIN_UV.head, 0, scale);
  paintPart(character.body, atlas, MC_SKIN_UV.body, 0, scale);
  paintPart(character.rightArm, atlas, MC_SKIN_UV.rightArm, 0, scale);
  paintPart(character.leftArm, atlas, MC_SKIN_UV.leftArm, 0, scale);
  paintPart(character.rightLeg, atlas, MC_SKIN_UV.rightLeg, 0, scale);
  paintPart(character.leftLeg, atlas, MC_SKIN_UV.leftLeg, 0, scale);

  paintPart(character.head, atlas, MC_SKIN_OVERLAY_UV.head, 1, scale, true);
  paintPart(character.body, atlas, MC_SKIN_OVERLAY_UV.body, 1, scale, true);
  paintPart(character.rightArm, atlas, MC_SKIN_OVERLAY_UV.rightArm, 1, scale, true);
  paintPart(character.leftArm, atlas, MC_SKIN_OVERLAY_UV.leftArm, 1, scale, true);
  paintPart(character.rightLeg, atlas, MC_SKIN_OVERLAY_UV.rightLeg, 1, scale, true);
  paintPart(character.leftLeg, atlas, MC_SKIN_OVERLAY_UV.leftLeg, 1, scale, true);
}

export function loadSkinImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.decoding = "async";
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`无法加载皮肤 ${url}`));
    image.src = url;
  });
}

export function cropSkinRegion(
  skin: HTMLImageElement | HTMLCanvasElement,
  rect: SkinRect,
): HTMLCanvasElement {
  const [sx, sy, sw, sh] = rect;
  const canvas = document.createElement("canvas");
  canvas.width = sw;
  canvas.height = sh;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("无法创建皮肤裁剪画布");
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(skin, sx, sy, sw, sh, 0, 0, sw, sh);
  return canvas;
}

function paintPart(
  box: Paintable,
  atlas: HTMLCanvasElement,
  uvs: Record<string, SkinRect>,
  layer: number,
  scale: number,
  skipEmpty = false,
): void {
  for (const face of [
    "front",
    "back",
    "left",
    "right",
    "top",
    "bottom",
  ] as const) {
    const base = uvs[face];
    if (base === undefined) continue;
    const [sx, sy, sw, sh] = scaleRect(base, scale);
    if (skipEmpty && regionIsFullyTransparent(atlas, sx, sy, sw, sh)) {
      try {
        box.paint(
          face,
          (ctx, canvas) => {
            ctx.clearRect(0, 0, canvas.width, canvas.height);
          },
          layer,
        );
      } catch {
        /* no layer */
      }
      continue;
    }
    try {
      box.paint(
        face,
        (ctx, canvas) => {
          ctx.imageSmoothingEnabled = false;
          ctx.clearRect(0, 0, canvas.width, canvas.height);
          // 用 nearest 放大到 canvas（segments 像素格）
          ctx.drawImage(
            atlas,
            sx,
            sy,
            sw,
            sh,
            0,
            0,
            canvas.width,
            canvas.height,
          );
        },
        layer,
      );
    } catch {
      /* Character 无对应 layer */
    }
  }
}

function regionIsFullyTransparent(
  atlas: HTMLCanvasElement,
  sx: number,
  sy: number,
  sw: number,
  sh: number,
): boolean {
  const ctx = atlas.getContext("2d");
  if (ctx === null) return true;
  const data = ctx.getImageData(sx, sy, sw, sh).data;
  for (let i = 3; i < data.length; i += 4) {
    if (data[i] > 8) return false;
  }
  return true;
}

function ensureCanvas(
  skin: HTMLImageElement | HTMLCanvasElement | ImageBitmap,
): HTMLCanvasElement {
  if (skin instanceof HTMLCanvasElement) return skin;
  const w =
    "naturalWidth" in skin
      ? skin.naturalWidth || skin.width
      : (skin as ImageBitmap).width;
  const h =
    "naturalHeight" in skin
      ? skin.naturalHeight || skin.height
      : (skin as ImageBitmap).height;
  const canvas = document.createElement("canvas");
  canvas.width = w || 64;
  canvas.height = h || 64;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("无法创建皮肤画布");
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(skin as CanvasImageSource, 0, 0);
  return canvas;
}

export function skinRegionTexture(
  skin: HTMLImageElement | HTMLCanvasElement,
  rect: SkinRect,
): Texture {
  const canvas = cropSkinRegion(skin, rect);
  const texture = new Texture(canvas);
  texture.colorSpace = SRGBColorSpace;
  texture.magFilter = NearestFilter;
  texture.minFilter = NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}
