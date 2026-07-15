/**
 * 单机天空盒内层绘制：透明清底 + 星点 + create.town 风格太阳。
 * 外层渐变由 World.sky.setShadingPhases 负责，内层绝不能整面实色。
 */

import type { World } from "@voxelize/core";
import { Color } from "three";

export interface LocalSkyStyle {
  backgroundColor: string;
  cloudsVisible: boolean;
  drawSun: boolean;
  drawStars: boolean;
  sky: {
    name: string;
    start: number;
    color: { top: string; middle: string; bottom: string };
    skyOffset: number;
    voidOffset: number;
  };
}

export function configureLocalAtmosphere(
  world: World,
  style: LocalSkyStyle,
): void {
  world.sky.visible = true;
  world.clouds.visible = style.cloudsVisible;
  // 单段锁定当前风格时段，避免跨多 phase 混色
  world.sky.setShadingPhases([
    {
      name: style.sky.name,
      start: 0,
      color: { ...style.sky.color },
      skyOffset: style.sky.skyOffset,
      voidOffset: style.sky.voidOffset,
    },
    {
      name: `${style.sky.name}-hold`,
      start: 1,
      color: { ...style.sky.color },
      skyOffset: style.sky.skyOffset,
      voidOffset: style.sky.voidOffset,
    },
  ]);
  // 颜色只靠外层 dodecahedron 渐变（setShadingPhases）。
  // 内层 CanvasBox 若铺整面不透明色，会变成「巨大紫色/红色贴纸」盖住渐变。
  world.sky.paint("all", clearSkyFace);
  if (style.drawStars) {
    world.sky.paint("top", paintStarsTransparent());
    world.sky.paint("sides", paintStarsTransparent());
    world.sky.paint("bottom", paintStarsTransparent());
  }
  if (style.drawSun) {
    world.sky.paint("bottom", (context, canvas) => {
      paintCreateTownStyleSun(context, canvas);
    });
  }
  world.background = new Color(style.backgroundColor);
}

/** 清空天空盒面为全透明，露出外层渐变 */
function clearSkyFace(
  context: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
): void {
  context.save();
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, canvas.width, canvas.height);
  context.restore();
}

/**
 * 透明底画星：只画点，不铺实色底，渐变天空透过 CanvasBox 可见。
 */
function paintStarsTransparent(
  starCount = 140,
): (context: CanvasRenderingContext2D, canvas: HTMLCanvasElement) => void {
  return (context, canvas) => {
    context.save();
    context.setTransform(1, 0, 0, 1, 0, 0);
    const colors = [
      "#FFFFFF",
      "#FFFFFF",
      "#FFE8E0",
      "#FFD0C8",
      "#E8E8FF",
      "#FF8585",
    ];
    for (let i = 0; i < starCount; i += 1) {
      context.globalAlpha = 0.45 + Math.random() * 0.55;
      context.beginPath();
      context.arc(
        Math.random() * canvas.width,
        Math.random() * canvas.height,
        Math.random() * 0.7 + 0.15,
        0,
        Math.PI * 2,
      );
      context.fillStyle = colors[Math.floor(Math.random() * colors.length)];
      context.fill();
    }
    context.restore();
  };
}

/** 按 lab 解包逻辑重绘太阳：低分 canvas + 径向辉光 + 实心核（原创实现）。 */
function paintCreateTownStyleSun(
  context: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
): void {
  context.save();
  context.imageSmoothingEnabled = false;
  const low = document.createElement("canvas");
  low.width = Math.max(32, Math.floor(canvas.width / 4));
  low.height = Math.max(32, Math.floor(canvas.height / 4));
  const lowContext = low.getContext("2d");
  if (lowContext === null) {
    context.restore();
    return;
  }
  lowContext.imageSmoothingEnabled = false;
  const cx = low.width / 2;
  const cy = low.height / 2;
  const glow = lowContext.createRadialGradient(cx, cy, 6.25, cx, cy, 31.25);
  glow.addColorStop(0, "rgba(255, 250, 200, 0.4)");
  glow.addColorStop(0.4, "rgba(255, 240, 150, 0.2)");
  glow.addColorStop(1, "rgba(255, 230, 100, 0)");
  lowContext.beginPath();
  lowContext.arc(cx, cy, 31.25, 0, Math.PI * 2);
  lowContext.fillStyle = glow;
  lowContext.fill();
  const core = lowContext.createRadialGradient(cx, cy, 0, cx, cy, 12.5);
  core.addColorStop(0, "rgb(255, 255, 245)");
  core.addColorStop(0.7, "rgb(255, 250, 220)");
  core.addColorStop(1, "rgb(255, 230, 140)");
  lowContext.beginPath();
  lowContext.arc(cx, cy, 12.5, 0, Math.PI * 2);
  lowContext.fillStyle = core;
  lowContext.fill();
  context.drawImage(low, 0, 0, canvas.width, canvas.height);
  context.restore();
}
