/**
 * 单机地图风格：生物群系（地表）× 时段（天空/光照）。
 * 每局由种子选取一套，保证可复现。
 */

export type LocalBiomeId =
  | "meadow"
  | "desert"
  | "snow"
  | "wasteland"
  | "rainforest"
  | "spring";

export type LocalDayPhaseId =
  | "day"
  | "sunset"
  | "night"
  | "midnight"
  | "blood";

export interface LocalSkyPhaseColors {
  top: string;
  middle: string;
  bottom: string;
}

export interface LocalMapStyle {
  /** 稳定 id，如 meadow_day */
  id: string;
  /** 中文展示名 */
  label: string;
  biome: LocalBiomeId;
  dayPhase: LocalDayPhaseId;
  /** 世界内时间 0–24000，驱动天空着色阶段 */
  worldTime: number;
  ambientIntensity: number;
  /** 环境光颜色（血月用偏红） */
  ambientColor: string;
  backgroundColor: string;
  minLightLevel: number;
  cloudsVisible: boolean;
  /** 天空着色阶段（单段锁定观感） */
  sky: {
    name: string;
    start: number;
    color: LocalSkyPhaseColors;
    skyOffset: number;
    voidOffset: number;
  };
  /** 是否绘制太阳圆盘 */
  drawSun: boolean;
  /** 是否绘制星空 */
  drawStars: boolean;
}

const WHITE = "#ffffff";

/** 精选风格：草地/荒漠/雪原/荒原/雨林 × 白昼/夜/黄昏/血月 */
export const LOCAL_MAP_STYLES: readonly LocalMapStyle[] = [
  {
    id: "meadow_day",
    label: "草地 · 白昼",
    biome: "meadow",
    dayPhase: "day",
    worldTime: 9_600,
    ambientIntensity: 0.35,
    ambientColor: WHITE,
    backgroundColor: "#4A90D9",
    minLightLevel: 0.06,
    cloudsVisible: true,
    sky: {
      name: "daylight",
      start: 0.25,
      color: { top: "#4A90D9", middle: "#7EC8E3", bottom: "#C9E4F6" },
      skyOffset: 0.2,
      voidOffset: 0.6,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "meadow_night",
    label: "草地 · 深夜",
    biome: "meadow",
    dayPhase: "night",
    worldTime: 18_500,
    ambientIntensity: 0.12,
    ambientColor: WHITE,
    backgroundColor: "#0a1020",
    minLightLevel: 0.1,
    cloudsVisible: true,
    sky: {
      name: "night",
      start: 0.88,
      color: { top: "#050814", middle: "#0c1224", bottom: "#151a2e" },
      skyOffset: 0.1,
      voidOffset: 0.55,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "meadow_blood",
    label: "草地 · 血月",
    biome: "meadow",
    dayPhase: "blood",
    worldTime: 20_000,
    ambientIntensity: 0.16,
    ambientColor: "#ff6a5a",
    backgroundColor: "#2a0808",
    minLightLevel: 0.12,
    cloudsVisible: true,
    sky: {
      name: "blood",
      start: 0.85,
      // 顶/侧/底同族血红，避免天空盒一面近黑、一面大红
      color: { top: "#4a1218", middle: "#5c181c", bottom: "#6a2024" },
      skyOffset: 0.1,
      voidOffset: 0.5,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "desert_day",
    label: "荒漠 · 烈日",
    biome: "desert",
    dayPhase: "day",
    worldTime: 11_000,
    ambientIntensity: 0.42,
    ambientColor: WHITE,
    backgroundColor: "#87b8e0",
    minLightLevel: 0.08,
    cloudsVisible: false,
    sky: {
      name: "daylight",
      start: 0.25,
      color: { top: "#5BA3D9", middle: "#F0D9A0", bottom: "#F5E6C8" },
      skyOffset: 0.22,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "desert_sunset",
    label: "荒漠 · 黄昏",
    biome: "desert",
    dayPhase: "sunset",
    worldTime: 15_500,
    ambientIntensity: 0.28,
    ambientColor: "#ffe0c0",
    backgroundColor: "#c45c2a",
    minLightLevel: 0.07,
    cloudsVisible: true,
    sky: {
      name: "sunset",
      start: 0.65,
      // 柔和顶色，避免 Canvas/渐变顶面过紫成「贴纸」
      color: { top: "#4a3868", middle: "#E85D3A", bottom: "#FFB86B" },
      skyOffset: 0.14,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "desert_blood",
    label: "荒漠 · 血月",
    biome: "desert",
    dayPhase: "blood",
    worldTime: 20_400,
    ambientIntensity: 0.18,
    ambientColor: "#ff7050",
    backgroundColor: "#301008",
    minLightLevel: 0.11,
    cloudsVisible: false,
    sky: {
      name: "blood",
      start: 0.86,
      color: { top: "#5a1810", middle: "#6e2014", bottom: "#7a2818" },
      skyOffset: 0.12,
      voidOffset: 0.5,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "snow_day",
    label: "雪原 · 白昼",
    biome: "snow",
    dayPhase: "day",
    worldTime: 8_000,
    ambientIntensity: 0.4,
    ambientColor: WHITE,
    backgroundColor: "#b8d4ea",
    minLightLevel: 0.08,
    cloudsVisible: true,
    sky: {
      name: "daylight",
      start: 0.25,
      color: { top: "#8eb8d8", middle: "#d0e4f2", bottom: "#f2f7fb" },
      skyOffset: 0.18,
      voidOffset: 0.5,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "snow_night",
    label: "雪原 · 寒夜",
    biome: "snow",
    dayPhase: "night",
    worldTime: 19_200,
    ambientIntensity: 0.14,
    ambientColor: "#c8d8f0",
    backgroundColor: "#0e1520",
    minLightLevel: 0.11,
    cloudsVisible: true,
    sky: {
      name: "night",
      start: 0.88,
      color: { top: "#060a12", middle: "#121a28", bottom: "#1c2434" },
      skyOffset: 0.1,
      voidOffset: 0.5,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "wasteland_sunset",
    label: "荒原 · 暮色",
    biome: "wasteland",
    dayPhase: "sunset",
    worldTime: 16_200,
    ambientIntensity: 0.22,
    ambientColor: "#ffd0b0",
    backgroundColor: "#4a3028",
    minLightLevel: 0.09,
    cloudsVisible: true,
    sky: {
      name: "twilight",
      start: 0.75,
      color: { top: "#1a1218", middle: "#5a3030", bottom: "#8a5040" },
      skyOffset: 0.1,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "wasteland_midnight",
    label: "荒原 · 子夜",
    biome: "wasteland",
    dayPhase: "midnight",
    worldTime: 22_000,
    ambientIntensity: 0.09,
    ambientColor: WHITE,
    backgroundColor: "#050508",
    minLightLevel: 0.12,
    cloudsVisible: false,
    sky: {
      name: "night",
      start: 0.9,
      color: { top: "#000000", middle: "#05050a", bottom: "#0a0a12" },
      skyOffset: 0.08,
      voidOffset: 0.5,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "wasteland_blood",
    label: "荒原 · 血月",
    biome: "wasteland",
    dayPhase: "blood",
    worldTime: 21_000,
    ambientIntensity: 0.14,
    ambientColor: "#ff4a3a",
    backgroundColor: "#1a0404",
    minLightLevel: 0.13,
    cloudsVisible: false,
    sky: {
      name: "blood",
      start: 0.88,
      color: { top: "#401010", middle: "#501414", bottom: "#5c1818" },
      skyOffset: 0.08,
      voidOffset: 0.48,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "rainforest_day",
    label: "雨林 · 白昼",
    biome: "rainforest",
    dayPhase: "day",
    worldTime: 10_200,
    ambientIntensity: 0.32,
    ambientColor: "#e8ffe8",
    backgroundColor: "#3a7a90",
    minLightLevel: 0.07,
    cloudsVisible: true,
    sky: {
      name: "daylight",
      start: 0.25,
      color: { top: "#2f6f8a", middle: "#5aa888", bottom: "#a8d4b0" },
      skyOffset: 0.16,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "rainforest_night",
    label: "雨林 · 夜雾",
    biome: "rainforest",
    dayPhase: "night",
    worldTime: 18_800,
    ambientIntensity: 0.11,
    ambientColor: "#a0d0c0",
    backgroundColor: "#061410",
    minLightLevel: 0.11,
    cloudsVisible: true,
    sky: {
      name: "night",
      start: 0.88,
      color: { top: "#040c0a", middle: "#0a1a16", bottom: "#102820" },
      skyOffset: 0.09,
      voidOffset: 0.5,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "rainforest_blood",
    label: "雨林 · 血月",
    biome: "rainforest",
    dayPhase: "blood",
    worldTime: 20_600,
    ambientIntensity: 0.15,
    ambientColor: "#ff5a48",
    backgroundColor: "#1a0808",
    minLightLevel: 0.12,
    cloudsVisible: true,
    sky: {
      name: "blood",
      start: 0.86,
      color: { top: "#3a1418", middle: "#4a181c", bottom: "#541c20" },
      skyOffset: 0.1,
      voidOffset: 0.48,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "snow_blood",
    label: "雪原 · 血月",
    biome: "snow",
    dayPhase: "blood",
    worldTime: 20_200,
    ambientIntensity: 0.15,
    ambientColor: "#ff7a6a",
    backgroundColor: "#180808",
    minLightLevel: 0.12,
    cloudsVisible: true,
    sky: {
      name: "blood",
      start: 0.87,
      color: { top: "#401418", middle: "#50181c", bottom: "#5c1c22" },
      skyOffset: 0.1,
      voidOffset: 0.48,
    },
    drawSun: false,
    drawStars: true,
  },
  {
    id: "spring_day",
    label: "春天 · 晴空",
    biome: "spring",
    dayPhase: "day",
    worldTime: 9_200,
    ambientIntensity: 0.38,
    ambientColor: "#f0fff0",
    backgroundColor: "#6ab4e8",
    minLightLevel: 0.07,
    cloudsVisible: true,
    sky: {
      name: "daylight",
      start: 0.25,
      color: { top: "#5aa8e8", middle: "#9ad4f0", bottom: "#d8f0c8" },
      skyOffset: 0.2,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "spring_sunset",
    label: "春天 · 黄昏",
    biome: "spring",
    dayPhase: "sunset",
    worldTime: 15_800,
    ambientIntensity: 0.3,
    ambientColor: "#ffe8d0",
    backgroundColor: "#e88850",
    minLightLevel: 0.07,
    cloudsVisible: true,
    sky: {
      name: "sunset",
      start: 0.65,
      // 柔和春日黄昏，避免顶面过紫成「贴纸」
      color: { top: "#4a3a68", middle: "#e87858", bottom: "#ffd090" },
      skyOffset: 0.14,
      voidOffset: 0.55,
    },
    drawSun: true,
    drawStars: false,
  },
  {
    id: "spring_night",
    label: "春天 · 夜色",
    biome: "spring",
    dayPhase: "night",
    worldTime: 19_000,
    ambientIntensity: 0.13,
    ambientColor: "#c8e0d8",
    backgroundColor: "#0a1420",
    minLightLevel: 0.1,
    cloudsVisible: true,
    sky: {
      name: "night",
      start: 0.88,
      color: { top: "#060e18", middle: "#101c2a", bottom: "#182838" },
      skyOffset: 0.1,
      voidOffset: 0.52,
    },
    drawSun: false,
    drawStars: true,
  },
] as const;

/** 由地图种子稳定挑选风格 */
export function pickMapStyle(seed: number): LocalMapStyle {
  const index = Math.abs(seed >>> 0) % LOCAL_MAP_STYLES.length;
  return LOCAL_MAP_STYLES[index];
}

export function getMapStyleById(id: string): LocalMapStyle | null {
  return LOCAL_MAP_STYLES.find((style) => style.id === id) ?? null;
}
