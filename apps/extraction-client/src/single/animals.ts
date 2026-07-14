/**
 * 单机主题动物：按生物群系刷少量可漫步的 Creature。
 * 纯装饰，无攻击/掉落；位置由种子可复现。
 */

import { Creature } from "@voxelize/core";
import type { Object3D } from "three";

import {
  LOCAL_EXTRACTION_XZ,
  LOCAL_SPAWN_XZ,
} from "./map-layout";
import {
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
} from "./map";
import type { LocalBiomeId } from "./map-style";

export type LocalAnimalKind =
  | "rabbit"
  | "sheep"
  | "camel"
  | "wolf"
  | "scavenger"
  | "parrot"
  | "deer"
  | "duck";

export interface LocalAnimalPalette {
  kind: LocalAnimalKind;
  label: string;
  head: string;
  face: string;
  body: string;
  legs: string;
  /** 相对默认 Creature 的整体缩放 */
  scale: number;
}

export interface LocalAnimalPlan {
  kind: LocalAnimalKind;
  label: string;
  x: number;
  y: number;
  z: number;
  palette: LocalAnimalPalette;
}

interface WanderingAnimal {
  creature: Creature;
  kind: LocalAnimalKind;
  targetX: number;
  targetZ: number;
  speed: number;
  retargetIn: number;
}

const BIOME_ANIMALS: Readonly<
  Record<LocalBiomeId, readonly LocalAnimalPalette[]>
> = {
  spring: [
    {
      kind: "rabbit",
      label: "野兔",
      head: "#e8d8c8",
      face: "#f0e8dc",
      body: "#d8c8b0",
      legs: "#c8b898",
      scale: 0.55,
    },
    {
      kind: "sheep",
      label: "绵羊",
      head: "#f4f0e8",
      face: "#e0d8c8",
      body: "#f8f4ec",
      legs: "#5a4a3a",
      scale: 0.85,
    },
    {
      kind: "duck",
      label: "野鸭",
      head: "#2a6a3a",
      face: "#f0c040",
      body: "#3a7a48",
      legs: "#d09030",
      scale: 0.5,
    },
    {
      kind: "deer",
      label: "小鹿",
      head: "#b88858",
      face: "#d0a878",
      body: "#a87848",
      legs: "#8a6038",
      scale: 0.9,
    },
  ],
  meadow: [
    {
      kind: "sheep",
      label: "绵羊",
      head: "#f0ece4",
      face: "#e0d8c8",
      body: "#f4f0e8",
      legs: "#4a3a2a",
      scale: 0.9,
    },
    {
      kind: "deer",
      label: "鹿",
      head: "#a07040",
      face: "#c09060",
      body: "#8a6038",
      legs: "#6a4828",
      scale: 1,
    },
    {
      kind: "rabbit",
      label: "兔子",
      head: "#c8b8a0",
      face: "#d8c8b0",
      body: "#b8a888",
      legs: "#a89878",
      scale: 0.5,
    },
  ],
  desert: [
    {
      kind: "camel",
      label: "骆驼",
      head: "#c9a06a",
      face: "#e0c090",
      body: "#b88850",
      legs: "#9a7040",
      scale: 1.15,
    },
    {
      kind: "scavenger",
      label: "沙蜥",
      head: "#8a7a40",
      face: "#a09050",
      body: "#7a6a38",
      legs: "#6a5a30",
      scale: 0.45,
    },
  ],
  snow: [
    {
      kind: "wolf",
      label: "灰狼",
      head: "#c8d0d8",
      face: "#e0e8f0",
      body: "#b0b8c0",
      legs: "#9098a0",
      scale: 0.95,
    },
    {
      kind: "rabbit",
      label: "雪兔",
      head: "#f0f4f8",
      face: "#ffffff",
      body: "#e8ecf0",
      legs: "#d0d4d8",
      scale: 0.5,
    },
  ],
  wasteland: [
    {
      kind: "scavenger",
      label: "食腐兽",
      head: "#5a4838",
      face: "#6a5840",
      body: "#4a3a2a",
      legs: "#3a2a1a",
      scale: 0.75,
    },
    {
      kind: "wolf",
      label: "荒狼",
      head: "#6a5a4a",
      face: "#7a6a58",
      body: "#5a4a3a",
      legs: "#4a3a2a",
      scale: 0.9,
    },
  ],
  rainforest: [
    {
      kind: "parrot",
      label: "鹦鹉",
      head: "#e04040",
      face: "#f0c020",
      body: "#20a050",
      legs: "#e07020",
      scale: 0.45,
    },
    {
      kind: "deer",
      label: "林鹿",
      head: "#6a5030",
      face: "#8a6840",
      body: "#5a4028",
      legs: "#4a3020",
      scale: 0.85,
    },
    {
      kind: "rabbit",
      label: "斑兔",
      head: "#4a6a3a",
      face: "#6a8a50",
      body: "#3a5a30",
      legs: "#2a4a28",
      scale: 0.5,
    },
  ],
};

const COUNT_BY_BIOME: Readonly<Record<LocalBiomeId, number>> = {
  spring: 8,
  meadow: 6,
  desert: 4,
  snow: 5,
  wasteland: 4,
  rainforest: 7,
};

/** 由种子规划动物落点（不依赖 Three），便于单测 */
export function planBiomeAnimals(
  seed: number,
  biome: LocalBiomeId,
  surfaceY: (x: number, z: number) => number,
  isBlocked?: (x: number, z: number) => boolean,
): LocalAnimalPlan[] {
  const palettes = BIOME_ANIMALS[biome];
  const count = COUNT_BY_BIOME[biome];
  const plans: LocalAnimalPlan[] = [];
  let guard = 0;
  while (plans.length < count && guard < count * 50) {
    guard += 1;
    const i = plans.length + guard;
    const x =
      LOCAL_WORLD_MIN +
      3 +
      Math.floor(hash01(i, seed, seed ^ 0xa11) * (LOCAL_WORLD_MAX - LOCAL_WORLD_MIN - 6));
    const z =
      LOCAL_WORLD_MIN +
      3 +
      Math.floor(hash01(seed, i, seed ^ 0xb22) * (LOCAL_WORLD_MAX - LOCAL_WORLD_MIN - 6));
    if (Math.hypot(x - LOCAL_SPAWN_XZ[0], z - LOCAL_SPAWN_XZ[1]) < 6) continue;
    if (Math.hypot(x - LOCAL_EXTRACTION_XZ[0], z - LOCAL_EXTRACTION_XZ[1]) < 5) {
      continue;
    }
    if (isBlocked?.(x, z)) continue;
    if (plans.some((p) => Math.hypot(p.x - x, p.z - z) < 3.5)) continue;
    const palette = palettes[Math.floor(hash01(x, z, seed) * palettes.length)];
    const y = surfaceY(x, z) + 1;
    plans.push({
      kind: palette.kind,
      label: palette.label,
      x: x + 0.5,
      y,
      z: z + 0.5,
      palette,
    });
  }
  return plans;
}

/**
 * 场景中的漫步动物群。
 * surfaceY / isBlocked 由 runtime 注入，便于贴地与避水。
 */
export class LocalAnimalSystem {
  private readonly animals: WanderingAnimal[] = [];
  private readonly seed: number;

  constructor(
    private readonly scene: Object3D,
    plans: readonly LocalAnimalPlan[],
    private readonly surfaceY: (x: number, z: number) => number,
    private readonly isBlocked: (x: number, z: number) => boolean,
    seed: number,
  ) {
    this.seed = seed >>> 0;
    for (const plan of plans) {
      this.animals.push(this.spawnOne(plan));
    }
  }

  update(deltaMs: number): void {
    const dt = Math.min(0.05, deltaMs / 1000);
    for (const animal of this.animals) {
      animal.retargetIn -= deltaMs;
      if (animal.retargetIn <= 0) {
        this.pickTarget(animal);
      }
      const pos = animal.creature.position;
      const dx = animal.targetX - pos.x;
      const dz = animal.targetZ - pos.z;
      const dist = Math.hypot(dx, dz);
      if (dist < 0.35) {
        animal.retargetIn = 0;
        animal.creature.speed = 0;
        animal.creature.update();
        continue;
      }
      const step = animal.speed * dt;
      const nx = pos.x + (dx / dist) * step;
      const nz = pos.z + (dz / dist) * step;
      const ix = Math.floor(nx);
      const iz = Math.floor(nz);
      if (
        ix < LOCAL_WORLD_MIN ||
        ix > LOCAL_WORLD_MAX ||
        iz < LOCAL_WORLD_MIN ||
        iz > LOCAL_WORLD_MAX ||
        this.isBlocked(ix, iz)
      ) {
        animal.retargetIn = 0;
        animal.creature.speed = 0;
        animal.creature.update();
        continue;
      }
      const ground = this.surfaceY(ix, iz) + 1;
      // 陡坡不硬穿
      if (Math.abs(ground - pos.y) > 1.6) {
        animal.retargetIn = 0;
        animal.creature.update();
        continue;
      }
      animal.creature.set(
        [nx, ground, nz],
        [dx / dist, 0, dz / dist],
      );
      animal.creature.manualSpeed = true;
      animal.creature.speed = animal.speed;
      animal.creature.update();
    }
  }

  dispose(): void {
    for (const animal of this.animals) {
      this.scene.remove(animal.creature);
    }
    this.animals.length = 0;
  }

  get count(): number {
    return this.animals.length;
  }

  private spawnOne(plan: LocalAnimalPlan): WanderingAnimal {
    const creature = new Creature({
      walkingSpeed: 0.6,
      positionLerp: 0.55,
      rotationLerp: 0.18,
      idleLegSwing: 0.02,
      head: {
        color: plan.palette.head,
        faceColor: plan.palette.face,
      },
      body: {
        color: plan.palette.body,
      },
      legs: {
        color: plan.palette.legs,
      },
    });
    creature.username = "";
    creature.scale.setScalar(plan.palette.scale);
    creature.position.set(plan.x, plan.y, plan.z);
    creature.newPosition.set(plan.x, plan.y, plan.z);
    this.scene.add(creature);
    const animal: WanderingAnimal = {
      creature,
      kind: plan.kind,
      targetX: plan.x,
      targetZ: plan.z,
      speed: plan.kind === "rabbit" || plan.kind === "duck" ? 1.4 : 0.9,
      retargetIn: 500 + hash01(plan.x * 10, plan.z * 10, this.seed) * 2000,
    };
    this.pickTarget(animal);
    return animal;
  }

  private pickTarget(animal: WanderingAnimal): void {
    const pos = animal.creature.position;
    for (let attempt = 0; attempt < 12; attempt += 1) {
      const angle = hash01(attempt, Math.floor(pos.x * 3), this.seed) * Math.PI * 2;
      const dist = 3 + hash01(Math.floor(pos.z * 3), attempt, this.seed) * 8;
      const tx = pos.x + Math.cos(angle) * dist;
      const tz = pos.z + Math.sin(angle) * dist;
      const ix = Math.floor(tx);
      const iz = Math.floor(tz);
      if (
        ix < LOCAL_WORLD_MIN + 1 ||
        ix > LOCAL_WORLD_MAX - 1 ||
        iz < LOCAL_WORLD_MIN + 1 ||
        iz > LOCAL_WORLD_MAX - 1
      ) {
        continue;
      }
      if (this.isBlocked(ix, iz)) continue;
      animal.targetX = tx;
      animal.targetZ = tz;
      animal.retargetIn = 2500 + hash01(ix, iz, this.seed) * 4500;
      return;
    }
    animal.targetX = pos.x;
    animal.targetZ = pos.z;
    animal.retargetIn = 1500;
  }
}

function hash01(x: number, z: number, seed: number): number {
  let h = Math.imul(x | 0, 374_761_393) ^ Math.imul(z | 0, 668_265_263) ^ seed;
  h = Math.imul(h ^ (h >>> 13), 1_274_126_177);
  h = h ^ (h >>> 16);
  return (h >>> 0) / 4_294_967_295;
}
