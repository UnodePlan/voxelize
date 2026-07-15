/**
 * 假人动作演示：循环「走路 → 原地挖掘」，
 * 模拟多人对战中看到的对方第三人称表现。
 * 兼容 Voxelize Character 与 Veloren 体素人形。
 *
 * 击退后会把巡逻线段平移到当前位置，避免「弹回」瞬移。
 */

import type { Object3D } from "three";

import type { MannequinActor } from "./mannequin-actor";
import type { McHeldTool } from "./mc-held-item";

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
  /** 可变巡逻线段（击退/复活后平移） */
  private fromXZ: [number, number];
  private toXZ: [number, number];
  private wasKnocked = false;
  /** 存活：击倒后 false，复活后 true */
  private alive = true;

  constructor(private readonly opts: MannequinDemoOptions) {
    this.walkSpeed = opts.walkSpeed ?? 2.2;
    this.walkLegSeconds = opts.walkLegSeconds ?? 4;
    this.mineSeconds = opts.mineSeconds ?? 3.5;
    this.digInterval = opts.digIntervalSeconds ?? 0.45;
    this.eyeHeight = opts.character.eyeHeight;
    this.fromXZ = [opts.fromXZ[0], opts.fromXZ[1]];
    this.toXZ = [opts.toXZ[0], opts.toXZ[1]];
    // 起始站在 from，走路态手持
    this.applyPose(0, /*facingTo*/ true);
    this.applyHeldForPhase("walk");
    opts.character.snapToTarget();
  }

  get phaseName(): MannequinDemoPhase {
    return this.phase;
  }

  get isAlive(): boolean {
    return this.alive;
  }

  /**
   * 当前手持工具（有 heldItems 时）；用于击杀掉落。
   * 无配置则 null（不掉假道具）。
   */
  getHeldTool(): McHeldTool | null {
    if (this.opts.heldItems === undefined) return null;
    return this.phase === "mine" ? "pickaxe" : "sword";
  }

  /** 眼睛世界坐标（用于掉落点 / 复活锚点） */
  getEyePosition(): [number, number, number] {
    const p = this.opts.character.root.position;
    return [p.x, p.y, p.z];
  }

  /**
   * 击倒：隐藏、清空手持、清击退；本帧起不再巡逻。
   * @returns 死亡时持有的工具（供掉落）；无则 null
   */
  kill(): McHeldTool | null {
    const held = this.getHeldTool();
    this.alive = false;
    this.wasKnocked = false;
    this.opts.character.setHeldItem?.(null);
    this.opts.character.root.visible = false;
    // 清运动学速度，避免复活后残留
    this.opts.character.applyKnockback?.([0, 0, 0]);
    if (this.opts.character.clearKnockback !== undefined) {
      this.opts.character.clearKnockback();
    }
    return held;
  }

  /**
   * 在击杀点附近复活：新巡逻线以 (x,z) 为中心，满血外观由调用方管。
   */
  respawnNear(x: number, z: number, pathHalfLength = 2.5): void {
    this.alive = true;
    this.phase = "walk";
    this.phaseElapsed = 0;
    this.digCooldown = 0;
    this.walkT = 0;
    this.walkDir = 1;
    this.wasKnocked = false;
    this.fromXZ = [x - pathHalfLength, z];
    this.toXZ = [x + pathHalfLength, z];
    this.opts.character.root.visible = true;
    if (this.opts.character.clearKnockback !== undefined) {
      this.opts.character.clearKnockback();
    }
    this.applyHeldForPhase("walk");
    this.applyPose(0, true);
    this.opts.character.snapToTarget();
  }

  update(deltaMs: number): void {
    if (!this.alive) return;

    const knocked = this.opts.character.isKnockedBack?.() === true;
    // 击退刚结束：把巡逻线平移到落点，避免 set 回原路径造成瞬移
    if (this.wasKnocked && !knocked) {
      this.reanchorPathToCharacter();
    }
    this.wasKnocked = knocked;

    if (knocked) {
      // 击退中随地形更新贴地高度，并只做角色积分
      this.syncKnockbackGround();
      this.opts.character.update();
      return;
    }

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

  /**
   * 平移巡逻线段，使当前 walkT 对应点落在角色当前位置。
   * 击退结束后调用，续走不瞬移。
   */
  reanchorPathToCharacter(): void {
    const pos = this.opts.character.root.position;
    const [x0, z0] = this.fromXZ;
    const [x1, z1] = this.toXZ;
    const cx = x0 + (x1 - x0) * this.walkT;
    const cz = z0 + (z1 - z0) * this.walkT;
    const ox = pos.x - cx;
    const oz = pos.z - cz;
    this.fromXZ = [x0 + ox, z0 + oz];
    this.toXZ = [x1 + ox, z1 + oz];
    // 同步 target（与当前位置重合），避免下一帧 lerp 抽搐
    const facingTo = this.walkDir > 0;
    this.applyPose(this.walkT, facingTo);
  }

  private syncKnockbackGround(): void {
    const setGround = this.opts.character.setKnockbackGroundEyeY;
    if (setGround === undefined) return;
    const pos = this.opts.character.root.position;
    const feetY =
      this.opts.surfaceY(Math.floor(pos.x), Math.floor(pos.z)) + 1;
    setGround.call(this.opts.character, feetY + this.eyeHeight);
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
    const [x0, z0] = this.fromXZ;
    const [x1, z1] = this.toXZ;
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
    const [x0, z0] = this.fromXZ;
    const [x1, z1] = this.toXZ;
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
