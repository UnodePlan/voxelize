import type { LocalBiomeId } from "./map-style";

export interface HeightFieldOptions {
  baseHeight: number;
  maxHeight: number;
  worldMin: number;
  worldMax: number;
  spawnXZ: readonly [number, number];
  extractXZ: readonly [number, number];
}

/**
 * 按生物群系生成高度场。同主题内 seed 不同 → 幅度/相位/偏向不同，每局略有变化。
 *
 * - meadow：缓丘 + 脊线，中心盆地 / 出生台地
 * - desert：沙丘波纹 + 偶发桌山，整体更干更起伏
 * - snow：大尺度圆润山丘，少尖脊
 * - wasteland：碎裂峡谷 + 深坑，起伏剧烈
 * - rainforest：中高林丘 + 河谷下切，表面更碎
 * - spring：柔和缓丘 + 浅洼地
 */
export function createBiomeHeightFunction(
  seed: number,
  biome: LocalBiomeId,
  opts: HeightFieldOptions,
): (x: number, z: number) => number {
  const s = seed >>> 0;
  // 局内变化旋钮（同 biome 不同 seed）
  const amp = 0.72 + hash01(1, 7, s) * 0.6; // 0.72–1.32
  const detail = 0.65 + hash01(2, 7, s) * 0.7;
  const biasSign = hash01(3, 7, s) > 0.5 ? 1 : -1;
  const biasAmt = 0.02 + hash01(4, 7, s) * 0.05;
  const dunePhase = hash01(5, 7, s) * Math.PI * 2;
  const duneAngle = hash01(6, 7, s) * Math.PI;
  const duneFreq = 0.07 + hash01(8, 7, s) * 0.08;
  const cosA = Math.cos(duneAngle);
  const sinA = Math.sin(duneAngle);

  return (x: number, z: number) => {
    let h: number;
    switch (biome) {
      case "desert":
        h = heightDesert(
          x,
          z,
          s,
          opts.baseHeight,
          amp,
          detail,
          dunePhase,
          duneFreq,
          cosA,
          sinA,
        );
        break;
      case "snow":
        h = heightSnow(x, z, s, opts.baseHeight, amp, detail, biasSign, biasAmt);
        break;
      case "wasteland":
        h = heightWasteland(
          x,
          z,
          s,
          opts.baseHeight,
          amp,
          detail,
          biasSign,
          biasAmt,
        );
        break;
      case "rainforest":
        h = heightRainforest(
          x,
          z,
          s,
          opts.baseHeight,
          amp,
          detail,
          biasSign,
          biasAmt,
        );
        break;
      case "spring":
        h = heightSpring(
          x,
          z,
          s,
          opts.baseHeight,
          amp,
          detail,
          biasSign,
          biasAmt,
        );
        break;
      default:
        h = heightMeadow(
          x,
          z,
          s,
          opts.baseHeight,
          amp,
          detail,
          biasSign,
          biasAmt,
        );
        break;
    }

    h = applyPlayAnchors(h, x, z, biome, opts);
    h = applyEdgeFalloff(h, x, z, biome, opts);
    const y = Math.round(h);
    return Math.max(5, Math.min(opts.maxHeight - 6, y));
  };
}

// ——— 各 biome 主体形状 ———

/** 草地：缓丘 + 脊线 + 区域偏向 */
function heightMeadow(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  biasSign: number,
  biasAmt: number,
): number {
  const hills = fbm(x * 0.035, z * 0.035, seed);
  const ridge = 1 - Math.abs(fbm(x * 0.05, z * 0.05, seed ^ 0xa11) * 2 - 1);
  const micro = fbm(x * 0.12, z * 0.12, seed ^ 0xb0b);
  const bias = biasSign * (x + z) * biasAmt;
  return (
    base +
    (hills - 0.5) * 9 * amp +
    (ridge - 0.45) * 5 * amp +
    (micro - 0.5) * 2.2 * detail +
    bias
  );
}

/** 荒漠：沙丘波纹主导，偶发桌山平台 */
function heightDesert(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  dunePhase: number,
  duneFreq: number,
  cosA: number,
  sinA: number,
): number {
  const u = x * cosA + z * sinA;
  const v = -x * sinA + z * cosA;
  const dunes =
    Math.sin(u * duneFreq + dunePhase) * 4.2 * amp +
    Math.sin(u * duneFreq * 2.15 + dunePhase * 0.7) * 2.1 * amp +
    Math.sin(v * duneFreq * 0.55 + 0.9) * 1.4 * amp;
  const wind = (fbm(x * 0.04, z * 0.04, seed ^ 0xd55) - 0.5) * 3.5 * detail;
  const grit = (fbm(x * 0.14, z * 0.14, seed ^ 0xd77) - 0.5) * 1.2 * detail;
  let h = base - 0.5 + dunes + wind + grit;

  // 稀疏桌山：局部抬成平台
  const mesa = fbm(x * 0.06, z * 0.06, seed ^ 0xd99);
  if (mesa > 0.72) {
    const lift = (mesa - 0.72) / 0.28;
    h = Math.max(h, base + 3 + lift * 5 * amp);
  }
  return h;
}

/** 雪原：大尺度圆润起伏，少尖角 */
function heightSnow(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  biasSign: number,
  biasAmt: number,
): number {
  const broad = fbm(x * 0.022, z * 0.022, seed ^ 0x511);
  const soft = fbm(x * 0.045, z * 0.045, seed ^ 0x522);
  const powder = fbm(x * 0.1, z * 0.1, seed ^ 0x533);
  const bias = biasSign * (x * 0.6 - z * 0.4) * biasAmt;
  return (
    base +
    1.5 +
    (broad - 0.5) * 10 * amp +
    (soft - 0.5) * 4 * amp +
    (powder - 0.5) * 1.2 * detail +
    bias
  );
}

/** 荒原：高起伏 + 峡谷裂缝 + 深坑 */
function heightWasteland(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  biasSign: number,
  biasAmt: number,
): number {
  const jagged = fbm(x * 0.05, z * 0.05, seed ^ 0x711);
  const ridge = 1 - Math.abs(fbm(x * 0.07, z * 0.07, seed ^ 0x722) * 2 - 1);
  const chaos = fbm(x * 0.16, z * 0.16, seed ^ 0x733);
  const bias = biasSign * (x - z) * biasAmt * 1.4;
  let h =
    base -
    1 +
    (jagged - 0.5) * 14 * amp +
    (ridge - 0.4) * 8 * amp +
    (chaos - 0.5) * 3.5 * detail +
    bias;

  // 深坑
  const pit = fbm(x * 0.09, z * 0.09, seed ^ 0x744);
  if (pit < 0.28) {
    h -= (0.28 - pit) * 18 * amp;
  }
  // 裂谷条带
  const fault = Math.abs(
    Math.sin(x * 0.18 + fbm(x * 0.03, z * 0.03, seed) * 2) *
      0.5 +
      Math.cos(z * 0.14) * 0.5,
  );
  if (fault < 0.12) {
    h -= (0.12 - fault) * 22;
  }
  return h;
}

/** 春天：柔和缓丘 + 浅洼（给池塘/小溪留空间） */
function heightSpring(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  biasSign: number,
  biasAmt: number,
): number {
  const hills = fbm(x * 0.032, z * 0.032, seed ^ 0xa01);
  const soft = fbm(x * 0.06, z * 0.06, seed ^ 0xa02);
  const micro = fbm(x * 0.13, z * 0.13, seed ^ 0xa03);
  const bias = biasSign * (x * 0.5 + z * 0.5) * biasAmt * 0.7;
  let h =
    base +
    0.5 +
    (hills - 0.5) * 6.5 * amp +
    (soft - 0.5) * 3.2 * amp +
    (micro - 0.5) * 1.4 * detail +
    bias;

  // 浅洼地：局部下切 1–3 格
  const hollow = fbm(x * 0.05, z * 0.05, seed ^ 0xa11);
  if (hollow < 0.32) {
    h -= (0.32 - hollow) * 9;
  }
  // 轻缓小溪
  const brook = riverMask(x, z, seed ^ 0xa22);
  if (brook > 0.5) {
    h -= (brook - 0.5) * 5;
  }
  return h;
}

/** 雨林：林丘连绵 + 河谷下切 */
function heightRainforest(
  x: number,
  z: number,
  seed: number,
  base: number,
  amp: number,
  detail: number,
  biasSign: number,
  biasAmt: number,
): number {
  const canopy = fbm(x * 0.03, z * 0.03, seed ^ 0x911);
  const knoll = fbm(x * 0.07, z * 0.07, seed ^ 0x922);
  const root = fbm(x * 0.15, z * 0.15, seed ^ 0x933);
  const bias = biasSign * (z * 0.7 + x * 0.3) * biasAmt;
  let h =
    base +
    1 +
    (canopy - 0.5) * 7 * amp +
    (knoll - 0.5) * 4.5 * amp +
    (root - 0.5) * 2.8 * detail +
    bias;

  // 蜿蜒河谷：下切 2–4 格
  const river = riverMask(x, z, seed);
  if (river > 0.45) {
    h -= (river - 0.45) * 8;
  }
  return h;
}

// ——— 玩法锚点 / 边缘（各 biome 强度不同） ———

function applyPlayAnchors(
  h: number,
  x: number,
  z: number,
  biome: LocalBiomeId,
  opts: HeightFieldOptions,
): number {
  const [ex, ez] = opts.extractXZ;
  const [sx, sz] = opts.spawnXZ;
  const extractDist = Math.hypot(x - ex, z - ez);
  const spawnDist = Math.hypot(x - sx, z - sz);

  // 撤离盆地：荒原更深，荒漠更浅平；春天略浅以保留草地
  const extractR = biome === "wasteland" ? 9 : biome === "desert" ? 7 : 8;
  const extractTarget =
    opts.baseHeight +
    (biome === "wasteland"
      ? -2.5
      : biome === "snow"
        ? 0
        : biome === "spring"
          ? -0.5
          : -1);
  if (extractDist < extractR) {
    const t = 1 - extractDist / extractR;
    h = lerp(
      h,
      extractTarget,
      t * t * (biome === "rainforest" || biome === "spring" ? 0.75 : 0.9),
    );
  }

  // 出生台地
  const spawnR = biome === "desert" ? 7 : 6;
  const spawnTarget =
    opts.baseHeight +
    (biome === "snow"
      ? 3
      : biome === "rainforest" || biome === "spring"
        ? 1.5
        : 2);
  if (spawnDist < spawnR) {
    const t = 1 - spawnDist / spawnR;
    h = lerp(h, spawnTarget, t * t * 0.75);
  }
  return h;
}

function applyEdgeFalloff(
  h: number,
  x: number,
  z: number,
  biome: LocalBiomeId,
  opts: HeightFieldOptions,
): number {
  const edge = edgeFalloff01(x, z, opts.worldMin, opts.worldMax);
  // 荒漠边缘沙崩更陡；雪原边缘更缓
  const strength =
    biome === "desert" ? 10 : biome === "snow" ? 5 : biome === "wasteland" ? 9 : 7;
  return h - edge * edge * strength;
}

function edgeFalloff01(
  x: number,
  z: number,
  worldMin: number,
  worldMax: number,
): number {
  const nx = (x - worldMin) / Math.max(1, worldMax - worldMin);
  const nz = (z - worldMin) / Math.max(1, worldMax - worldMin);
  const dx = Math.min(nx, 1 - nx) * 2;
  const dz = Math.min(nz, 1 - nz) * 2;
  const centerish = Math.min(dx, dz);
  return Math.max(0, 1 - centerish / 0.35);
}

function riverMask(x: number, z: number, seed: number): number {
  const meander =
    Math.sin(x * 0.28 + z * 0.1 + fbm(x * 0.05, z * 0.05, seed ^ 0xcec) * 3.5) *
      0.5 +
    0.5;
  const dist = Math.abs(
    z -
      (-2 +
        Math.sin(x * 0.2) * 7 +
        (fbm(x * 0.07, 0, seed ^ 0xc11) - 0.5) * 5),
  );
  const ribbon = Math.max(0, 1 - dist / 2.4);
  return Math.max(ribbon, meander * 0.12);
}

// ——— 噪声（自包含，避免与 map.ts 循环依赖） ———

function fbm(x: number, z: number, seed: number): number {
  let amp = 1;
  let freq = 1;
  let sum = 0;
  let norm = 0;
  for (let octave = 0; octave < 4; octave += 1) {
    sum += amp * valueNoise(x * freq, z * freq, seed + octave * 1013);
    norm += amp;
    amp *= 0.5;
    freq *= 2;
  }
  return sum / norm;
}

function valueNoise(x: number, z: number, seed: number): number {
  const x0 = Math.floor(x);
  const z0 = Math.floor(z);
  const fx = smoothstep(x - x0);
  const fz = smoothstep(z - z0);
  const v00 = hash01(x0, z0, seed);
  const v10 = hash01(x0 + 1, z0, seed);
  const v01 = hash01(x0, z0 + 1, seed);
  const v11 = hash01(x0 + 1, z0 + 1, seed);
  return lerp(lerp(v00, v10, fx), lerp(v01, v11, fx), fz);
}

export function hash01(x: number, z: number, seed: number): number {
  let h = Math.imul(x | 0, 374_761_393) ^ Math.imul(z | 0, 668_265_263) ^ seed;
  h = Math.imul(h ^ (h >>> 13), 1_274_126_177);
  h = h ^ (h >>> 16);
  return (h >>> 0) / 4_294_967_295;
}

function smoothstep(t: number): number {
  return t * t * (3 - 2 * t);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}
