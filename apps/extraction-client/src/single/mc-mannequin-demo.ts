/**
 * 假人动作演示：循环「走路 → 原地挖掘」，
 * 模拟多人对战中看到的对方第三人称表现。
 * 兼容 Voxelize Character 与 Veloren 体素人形。
 */

import type { Object3D } from "three";

import type { MannequinActor } from "./mannequin-actor";

export type MannequinDemoPhase = "walk" | "mine";

export interface MannequinDemoHeldItems {
  /** 走路阶段（默认铁剑） */
  walk: Object3D;
  /** 挖掘阶段（默认铁镐） */
  mine: Object3D;
}

export interface MannequinDemoOptions {
  character: MannequinActor;
  /** 脚底贴地高度（世界 y = feetY + eyeHeight） */
  surfaceY: (x: number, z: number) => number;
  /** 巡逻线段两端（xz） */
  fromXZ: readonly [number, number];
  toXZ: readonly [number, number];
  /** 走路速度（格/秒） */
  walkSpeed?: number;
  /** 单程走路时长上限（秒），超时则掉头 */
  walkLegSeconds?: number;
  /** 挖掘阶段时长（秒） */
  mineSeconds?: number;
  /** 挖掘挥臂间隔（秒） */
  digIntervalSeconds?: number;
  /** 阶段切换时换手持物（需 character.setHeldItem） */
  heldItems?: MannequinDemoHeldItems;
}

/**
 * 驱动 Character 的位置/朝向与挥臂，需在每帧 ready 后调用 update。
 */
export class McMannequinDemo {
  private phase: MannequinDemoPhase = "walk";
  private phaseElapsed = 0;
  private digCooldown = 0;
  private walkT = 0; // 0→1 沿线插值
  private walkDir = 1; // +1 往 to，-1 往 from
  private readonly walkSpeed: number;
  private readonly walkLegSeconds: number;
  private readonly mineSeconds: number;
  private readonly digInterval: number;
  private readonly eyeHeight: number;

  constructor(private readonly opts: MannequinDemoOptions) {
    this.walkSpeed = opts.walkSpeed ?? 2.2;
    this.walkLegSeconds = opts.walkLegSeconds ?? 4;
    this.mineSeconds = opts.mineSeconds ?? 3.5;
    this.digInterval = opts.digIntervalSeconds ?? 0.45;
    this.eyeHeight = opts.character.eyeHeight;
    // 起始站在 from，走路态手持
    this.applyPose(0, /*facingTo*/ true);
    this.applyHeldForPhase("walk");
    opts.character.snapToTarget();
  }

  get phaseName(): MannequinDemoPhase {
    return this.phase;
  }

  update(deltaMs: number): void {
    // 大步长拆成 50ms 子步，保证阶段切换与测试可复现
    let remaining = Math.min(1, Math.max(0, deltaMs / 1000));
    while (remaining > 1e-6) {
      const dt = Math.min(0.05, remaining);
      remaining -= dt;
      this.phaseElapsed += dt;

      if (this.phase === "walk") {
        this.updateWalk(dt);
        // 浮点累计 0.05*10 可能略小于 0.5，放一点 epsilon
        if (this.phaseElapsed + 1e-6 >= this.walkLegSeconds) {
          this.enterMine();
        }
      } else {
        this.updateMine(dt);
        if (this.phaseElapsed + 1e-6 >= this.mineSeconds) {
          this.enterWalk();
        }
      }
    }

    this.opts.character.update();
  }

  private enterWalk(): void {
    this.phase = "walk";
    this.phaseElapsed = 0;
    this.walkDir *= -1;
    // 掉头时 walkT 保持，沿反方向走
    this.applyHeldForPhase("walk");
  }

  private enterMine(): void {
    this.phase = "mine";
    this.phaseElapsed = 0;
    this.digCooldown = 0;
    // 面向「下一段路」的反方向或固定朝西展示脸：朝向当前行走方向
    const facingTo = this.walkDir > 0;
    this.applyPose(this.walkT, facingTo);
    this.applyHeldForPhase("mine");
    // 立刻挥一下，进入挖掘观感
    this.opts.character.playArmSwingAnimation();
  }

  private applyHeldForPhase(phase: MannequinDemoPhase): void {
    const held = this.opts.heldItems;
    const setHeld = this.opts.character.setHeldItem;
    if (held === undefined || setHeld === undefined) return;
    setHeld.call(this.opts.character, phase === "mine" ? held.mine : held.walk);
  }

  private updateWalk(dt: number): void {
    const [x0, z0] = this.opts.fromXZ;
    const [x1, z1] = this.opts.toXZ;
    const pathLen = Math.hypot(x1 - x0, z1 - z0) || 1;
    const deltaT = (this.walkSpeed * dt) / pathLen;
    this.walkT += this.walkDir * deltaT;

    if (this.walkT >= 1) {
      this.walkT = 1;
      this.walkDir = -1;
    } else if (this.walkT <= 0) {
      this.walkT = 0;
      this.walkDir = 1;
    }

    const facingTo = this.walkDir > 0;
    this.applyPose(this.walkT, facingTo);
  }

  private updateMine(dt: number): void {
    // 站定：位置不变，但每帧 set 同一点 → speed=0 → 腿停；靠 playArmSwing 挥右臂
    const facingTo = this.walkDir > 0;
    this.applyPose(this.walkT, facingTo);

    this.digCooldown -= dt;
    if (this.digCooldown <= 0) {
      this.opts.character.playArmSwingAnimation();
      this.digCooldown = this.digInterval;
    }
  }

  private applyPose(t: number, facingTo: boolean): void {
    const [x0, z0] = this.opts.fromXZ;
    const [x1, z1] = this.opts.toXZ;
    const x = x0 + (x1 - x0) * t;
    const z = z0 + (z1 - z0) * t;
    const feetY = this.opts.surfaceY(Math.floor(x), Math.floor(z)) + 1;
    const eyeY = feetY + this.eyeHeight;

    // 朝向：沿路径方向（或反向）
    let dx = x1 - x0;
    let dz = z1 - z0;
    if (!facingTo) {
      dx = -dx;
      dz = -dz;
    }
    const len = Math.hypot(dx, dz) || 1;
    dx /= len;
    dz /= len;

    this.opts.character.set([x, eyeY, z], [dx, 0, dz]);
  }
}
