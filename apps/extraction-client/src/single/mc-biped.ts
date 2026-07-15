/**
 * 原版 MC ModelBiped / ModelPlayer 几何 + 动画（对照 1.8 源码 & skinview3d）。
 *
 * 单位：先按像素建盒子，整体 scale=1/16。
 * 枢轴：
 *   head (0,0,0)  local box 中心偏上
 *   body (0,-6)   相对 root
 *   arm  (±5,-2)
 *   leg  (±1.9,-12)
 * 动画：cos(limbSwing * 0.6662) …
 */

import {
  BoxGeometry,
  BufferAttribute,
  DoubleSide,
  FrontSide,
  Group,
  MathUtils,
  Mesh,
  MeshLambertMaterial,
  NearestFilter,
  SRGBColorSpace,
  Texture,
  Vector2,
  Vector3,
  type Object3D,
} from "three";

import defaultSkinUrl from "../assets/single/skins/steve_classic.png";

import type { MannequinActor } from "./mannequin-actor";
import { loadSkinImage } from "./mc-skin";

const PX = 1 / 16;
/** 脚底 → 眼（约头心）：ModelBiped 脚在 -24，头心约 +4 */
const EYE_FROM_FEET = 28 * PX; // 1.75
/**
 * 第一人称：身体相对眼位沿模型后向（+Z）后移的像素。
 * 眼睛在头前脸，头心/身体中轴在眼后，低头看到身下正面而非眼前假人。
 */
const FP_BODY_BACK_PX = 6;
/** 视线俯角门控：lookDir.y 小于此值才开始露出身下（平视不挡视野） */
const FP_LOOK_DOWN_Y = -0.22;
/**
 * 极低头：再往下藏躯干，避免视线打在 body 顶面看到「身体中心」。
 * 仍保留腿 + 两侧手臂。
 */
const FP_HIDE_TORSO_Y = -0.72;

export interface McBipedOptions {
  username?: string;
  skinUrl?: string;
  /** true = Alex 细臂 */
  slim?: boolean;
}

export async function createMcBipedMannequin(
  options: McBipedOptions = {},
): Promise<McBipedMannequin> {
  const skin = await loadSkinImage(options.skinUrl ?? defaultSkinUrl);
  const tex = imageToSkinTexture(skin);
  const biped = new McBipedMannequin(tex, options.slim ?? false);
  biped.username = options.username ?? "Steve";
  return biped;
}

export class McBipedMannequin implements MannequinActor {
  readonly root = new Group();
  /** 像素坐标系下的模型根（原点同 ModelBiped） */
  private readonly model = new Group();
  private readonly head = new Group();
  private readonly body = new Group();
  private readonly rightArm = new Group();
  private readonly leftArm = new Group();
  private readonly rightLeg = new Group();
  private readonly leftLeg = new Group();
  /** 右臂末端挂点：第三人称镐/剑（像素坐标） */
  private readonly heldSlot = new Group();

  private readonly targetPos = new Vector3();
  private targetYaw = 0;
  private currentYaw = 0;

  private limbSwing = 0;
  private limbSwingAmount = 0;
  private age = 0;
  private digProgress = -1; // -1 idle；0..1 swing
  private moving = false;
  /** 第一人称自身模式：身体前移 + 低头才显示躯干/腿 */
  private firstPersonSelf = false;

  /**
   * 运动学击退速度（格/秒）。demo 暂停 set 时由本类积分位移。
   * mass≈1 时与 RigidBody 冲量同量级。
   */
  private readonly kbVel = new Vector3();
  /** 击退期间贴地的眼睛高度（脚底 + eyeHeight） */
  private kbGroundEyeY = 0;

  readonly eyeHeight = EYE_FROM_FEET;
  username = "Steve";

  private readonly materials: MeshLambertMaterial[] = [];
  private readonly texture: Texture;

  constructor(texture: Texture, slim: boolean) {
    this.texture = texture;
    this.root.name = "mc-biped";
    this.model.name = "mc-biped-model";

    const mat1 = makeSkinMaterial(texture, false);
    const mat2 = makeSkinMaterial(texture, true);
    const mat1b = mat1.clone();
    mat1b.polygonOffset = true;
    mat1b.polygonOffsetFactor = 1;
    mat1b.polygonOffsetUnits = 1;
    const mat2b = mat2.clone();
    mat2b.polygonOffset = true;
    mat2b.polygonOffsetFactor = 1;
    mat2b.polygonOffsetUnits = 1;
    this.materials.push(mat1, mat2, mat1b, mat2b);

    // —— Head (inner 8³ + outer 9³ hat) ——
    const headInner = boxWithSkin(8, 8, 8, 0, 0, 8, 8, 8, mat1);
    headInner.position.y = 4;
    const headOuter = boxWithSkin(9, 9, 9, 32, 0, 8, 8, 8, mat2);
    headOuter.position.y = 4;
    this.head.add(headInner, headOuter);
    this.model.add(this.head);

    // —— Body ——
    const bodyInner = boxWithSkin(8, 12, 4, 16, 16, 8, 12, 4, mat1);
    const bodyOuter = boxWithSkin(8.5, 12.5, 4.5, 16, 32, 8, 12, 4, mat2);
    this.body.add(bodyInner, bodyOuter);
    this.body.position.y = -6;
    this.model.add(this.body);

    // —— Arms ——
    const armW = slim ? 3 : 4;
    const armW2 = slim ? 3.5 : 4.5;
    // Right
    const rArmIn = boxWithSkin(armW, 12, 4, 40, 16, armW, 12, 4, mat1b);
    const rArmOut = boxWithSkin(armW2, 12.5, 4.5, 40, 32, armW, 12, 4, mat2b);
    const rPivot = new Group();
    rPivot.position.set(slim ? -0.5 : -1, -4, 0);
    rPivot.add(rArmIn, rArmOut);
    this.rightArm.add(rPivot);
    // 手部附近：随 rightArm 旋转，供第三人称手持物
    this.heldSlot.name = "mc-held-slot";
    this.heldSlot.position.set(0, -10, -1);
    this.rightArm.add(this.heldSlot);
    this.rightArm.position.set(-5, -2, 0);
    this.model.add(this.rightArm);
    // Left
    const lArmIn = boxWithSkin(armW, 12, 4, 32, 48, armW, 12, 4, mat1b);
    const lArmOut = boxWithSkin(armW2, 12.5, 4.5, 48, 48, armW, 12, 4, mat2b);
    const lPivot = new Group();
    lPivot.position.set(slim ? 0.5 : 1, -4, 0);
    lPivot.add(lArmIn, lArmOut);
    this.leftArm.add(lPivot);
    this.leftArm.position.set(5, -2, 0);
    this.model.add(this.leftArm);

    // —— Legs ——
    const rLegIn = boxWithSkin(4, 12, 4, 0, 16, 4, 12, 4, mat1b);
    const rLegOut = boxWithSkin(4.5, 12.5, 4.5, 0, 32, 4, 12, 4, mat2b);
    const rLegPivot = new Group();
    rLegPivot.position.y = -6;
    rLegPivot.add(rLegIn, rLegOut);
    this.rightLeg.add(rLegPivot);
    this.rightLeg.position.set(-1.9, -12, -0.1);
    this.model.add(this.rightLeg);

    const lLegIn = boxWithSkin(4, 12, 4, 16, 48, 4, 12, 4, mat1b);
    const lLegOut = boxWithSkin(4.5, 12.5, 4.5, 0, 48, 4, 12, 4, mat2b);
    const lLegPivot = new Group();
    lLegPivot.position.y = -6;
    lLegPivot.add(lLegIn, lLegOut);
    this.leftLeg.add(lLegPivot);
    this.leftLeg.position.set(1.9, -12, -0.1);
    this.model.add(this.leftLeg);

    // 像素 → 方块；眼睛对齐 root 原点（ModelBiped 头心约 y=+4）
    this.model.scale.setScalar(PX);
    this.model.position.y = -4 * PX;
    this.root.add(this.model);
  }

  get rootObject(): Object3D {
    return this.root;
  }

  set(position: number[], direction: number[]): void {
    this.targetPos.set(position[0], position[1], position[2]);
    const dx = direction[0];
    const dz = direction[2];
    if (Math.abs(dx) + Math.abs(dz) > 1e-6) {
      // 模型局部 -Z 为前；对齐世界水平朝向
      this.targetYaw = yawFromLookXZ(dx, dz);
    }
    const dist =
      Math.hypot(
        position[0] - this.root.position.x,
        position[2] - this.root.position.z,
      ) + Math.abs(position[1] - this.root.position.y) * 0.1;
    this.moving = dist > 0.0008;
  }

  snapToTarget(): void {
    this.root.position.copy(this.targetPos);
    this.currentYaw = this.targetYaw;
    this.root.rotation.y = this.currentYaw;
  }

  /**
   * 第一人称自身：
   * - 永远隐藏头与双臂（手臂用 viewmodel；第三人称臂俯视会变成顶面大方块）
   * - 只露腿（中等低头可露躯干正面；极低头藏躯干避免顶面中心）
   * - 整体后移到眼后
   */
  enableFirstPersonSelfView(): void {
    this.firstPersonSelf = true;
    this.head.visible = false;
    // FP 不用身体手臂，避免俯视肩/臂顶面占满下半屏
    this.leftArm.visible = false;
    this.rightArm.visible = false;
    // 局部 +Z = 身后：身体在眼后
    this.model.position.z = FP_BODY_BACK_PX * PX;
    // 初始先藏，等 syncEyeFrame 按俯角打开
    this.body.visible = false;
    this.leftLeg.visible = false;
    this.rightLeg.visible = false;
  }

  /**
   * 每帧紧贴眼位与水平朝向（无插值，避免自身身体漂移）。
   * @param lookDir 世界空间视线方向（可含 pitch；水平用于 yaw，y 用于低头门控）
   */
  syncEyeFrame(
    position: Vector3,
    lookDir: Vector3,
    moving: boolean,
  ): void {
    this.root.position.copy(position);
    this.targetPos.copy(position);
    const dx = lookDir.x;
    const dz = lookDir.z;
    if (Math.abs(dx) + Math.abs(dz) > 1e-6) {
      this.targetYaw = yawFromLookXZ(dx, dz);
      this.currentYaw = this.targetYaw;
      this.root.rotation.y = this.currentYaw;
    }
    this.moving = moving;

    if (this.firstPersonSelf) {
      const lookDown = lookDir.y < FP_LOOK_DOWN_Y;
      // 极低头藏躯干顶面；臂始终关（viewmodel 负责手）
      const showTorso = lookDown && lookDir.y >= FP_HIDE_TORSO_Y;
      this.leftLeg.visible = lookDown;
      this.rightLeg.visible = lookDown;
      this.body.visible = showTorso;
      this.leftArm.visible = false;
      this.rightArm.visible = false;
    }
  }

  playArmSwingAnimation(): void {
    this.digProgress = 0;
  }

  /**
   * 受击击退：叠加速度，记录贴地眼高；demo 侧见 isKnockedBack 暂停巡逻。
   * 零向量仅用于兼容调用，实际清零请用 clearKnockback。
   */
  applyKnockback(impulse: readonly [number, number, number]): void {
    const ix = impulse[0];
    const iy = impulse[1];
    const iz = impulse[2];
    if (Math.abs(ix) + Math.abs(iy) + Math.abs(iz) < 1e-8) return;
    this.kbVel.x += ix;
    this.kbVel.y += iy;
    this.kbVel.z += iz;
    // 以当前眼高为地面参考，避免击飞后穿地
    this.kbGroundEyeY = this.root.position.y;
    // 立即同步 target，避免下一帧 lerp 把人拽回
    this.targetPos.copy(this.root.position);
    this.moving = true;
  }

  isKnockedBack(): boolean {
    return this.kbVel.lengthSq() > 0.04; // ~0.2 格/秒
  }

  clearKnockback(): void {
    this.kbVel.set(0, 0, 0);
  }

  setKnockbackGroundEyeY(eyeY: number): void {
    if (Number.isFinite(eyeY)) this.kbGroundEyeY = eyeY;
  }

  /**
   * 挂到右臂末端的第三人称手持物。传入 null 清空。
   * 不负责 dispose 旧 mesh（由调用方持有生命周期）。
   */
  setHeldItem(item: Object3D | null): void {
    while (this.heldSlot.children.length > 0) {
      this.heldSlot.remove(this.heldSlot.children[0]!);
    }
    if (item !== null) this.heldSlot.add(item);
  }

  update(): void {
    const dt = 1 / 60;
    this.age += dt;

    if (this.isKnockedBack()) {
      // 运动学积分：重力 + 贴地摩擦；不走 target lerp
      this.root.position.x += this.kbVel.x * dt;
      this.root.position.y += this.kbVel.y * dt;
      this.root.position.z += this.kbVel.z * dt;
      this.kbVel.y -= 22 * dt;
      if (this.root.position.y <= this.kbGroundEyeY) {
        this.root.position.y = this.kbGroundEyeY;
        if (this.kbVel.y < 0) this.kbVel.y = 0;
        // 贴地水平阻尼（类似摩擦）
        this.kbVel.x *= 0.82;
        this.kbVel.z *= 0.82;
      } else {
        this.kbVel.x *= 0.98;
        this.kbVel.z *= 0.98;
      }
      if (this.kbVel.lengthSq() < 0.04) {
        this.kbVel.set(0, 0, 0);
      }
      this.targetPos.copy(this.root.position);
      this.moving = this.kbVel.lengthSq() > 0.04;
    } else {
      // 位姿（第一人称 syncEyeFrame 已硬贴；第三人称仍插值）
      this.root.position.lerp(this.targetPos, 0.4);
    }

    // 最短角插值
    let dy = this.targetYaw - this.currentYaw;
    while (dy > Math.PI) dy -= Math.PI * 2;
    while (dy < -Math.PI) dy += Math.PI * 2;
    this.currentYaw += dy * 0.25;
    this.root.rotation.y = this.currentYaw;

    // limbSwing / amount（原版 setRotationAngles）
    const targetAmount = this.moving ? 1 : 0;
    this.limbSwingAmount = MathUtils.lerp(
      this.limbSwingAmount,
      targetAmount,
      0.3,
    );
    if (this.moving) {
      this.limbSwing += dt * 6.5; // 步频
    }

    const t = this.limbSwing * 0.6662;
    const a = this.limbSwingAmount;

    // 腿
    this.rightLeg.rotation.x = Math.cos(t) * 1.4 * a;
    this.leftLeg.rotation.x = Math.cos(t + Math.PI) * 1.4 * a;

    // 臂（走路）
    let rightArmX = Math.cos(t + Math.PI) * 2 * a * 0.5;
    let leftArmX = Math.cos(t) * 2 * a * 0.5;
    let rightArmZ = 0;
    let leftArmZ = 0;
    let rightArmY = 0;

    // 静止微摆（原版 age 项）
    rightArmZ += Math.cos(this.age * 0.09) * 0.05 + 0.05;
    leftArmZ -= Math.cos(this.age * 0.09) * 0.05 + 0.05;
    rightArmX += Math.sin(this.age * 0.067) * 0.05;
    leftArmX -= Math.sin(this.age * 0.067) * 0.05;

    // 挖掘 swingProgress（原版简化）
    if (this.digProgress >= 0) {
      this.digProgress = Math.min(1, this.digProgress + dt * 2.8);
      let sp = this.digProgress;
      let bodyY = Math.sin(Math.sqrt(sp) * Math.PI * 2) * 0.2;
      this.body.rotation.y = bodyY;

      sp = 1 - this.digProgress;
      sp = sp * sp * sp;
      sp = 1 - sp;
      const swing = Math.sin(sp * Math.PI);
      const headPitch = 0;
      const extra =
        Math.sin(this.digProgress * Math.PI) *
        -(headPitch - 0.7) *
        0.75;
      rightArmX = rightArmX - (swing * 1.2 + extra);
      rightArmY += bodyY * 2;
      rightArmZ += Math.sin(this.digProgress * Math.PI) * -0.4;

      if (this.digProgress >= 1) {
        this.digProgress = -1;
        this.body.rotation.y = 0;
      }
    } else {
      this.body.rotation.y = MathUtils.lerp(this.body.rotation.y, 0, 0.2);
    }

    this.rightArm.rotation.x = rightArmX;
    this.rightArm.rotation.y = rightArmY;
    this.rightArm.rotation.z = rightArmZ;
    this.leftArm.rotation.x = leftArmX;
    this.leftArm.rotation.z = leftArmZ;
  }

  dispose(): void {
    this.model.traverse((obj) => {
      if (obj instanceof Mesh) {
        obj.geometry.dispose();
      }
    });
    for (const m of this.materials) m.dispose();
    this.texture.dispose();
  }
}

// ——— 几何 / UV（与 skinview3d setUVs 一致，皮肤按 64 基准；128 图用 repeat） ———

/** 模型局部 -Z 朝前时，将世界水平视线 (dx,dz) 转为 root.rotation.y */
export function yawFromLookXZ(dx: number, dz: number): number {
  return Math.atan2(-dx, -dz);
}

function makeSkinMaterial(
  map: Texture,
  overlay: boolean,
): MeshLambertMaterial {
  return new MeshLambertMaterial({
    map,
    side: overlay ? DoubleSide : FrontSide,
    transparent: overlay,
    alphaTest: overlay ? 1e-5 : 0,
  });
}

function imageToSkinTexture(image: HTMLImageElement): Texture {
  // 统一画到 64×64 逻辑 UV：若是 128 则缩到 64 供 setSkinUVs；细节已在 128 源图，
  // 这里用 canvas 保持 nearest 缩到 64 会损失 HD——改为 texture 直接用原图 + UV /64 坐标。
  // skinview 固定 64：我们把 128 图当 64 用时需 map.repeat。
  const texture = new Texture(image);
  texture.colorSpace = SRGBColorSpace;
  texture.magFilter = NearestFilter;
  texture.minFilter = NearestFilter;
  texture.generateMipmaps = false;
  // 128 皮肤：UV 仍按 64 布局写，用 repeat=0.5 映射到半张？不对。
  // 正确：setUVs 用 textureWidth=image.width
  texture.needsUpdate = true;
  (texture as Texture & { __skinW?: number }).__skinW =
    image.naturalWidth || image.width || 64;
  (texture as Texture & { __skinH?: number }).__skinH =
    image.naturalHeight || image.height || 64;
  return texture;
}

function boxWithSkin(
  w: number,
  h: number,
  d: number,
  u: number,
  v: number,
  skinW: number,
  skinH: number,
  skinD: number,
  material: MeshLambertMaterial,
): Mesh {
  const geo = new BoxGeometry(w, h, d);
  const tex = material.map as Texture & { __skinW?: number; __skinH?: number };
  const tw = tex?.__skinW ?? 64;
  const th = tex?.__skinH ?? 64;
  // 64 基准 UV → 若皮肤 128，坐标 ×2
  const scale = tw >= 128 ? tw / 64 : 1;
  setSkinUVs(
    geo,
    u * scale,
    v * scale,
    skinW * scale,
    skinH * scale,
    skinD * scale,
    tw,
    th,
  );
  return new Mesh(geo, material);
}

/**
 * Magica/MC 盒子 UV 展开（与 skinview3d 相同）。
 * 面顺序：right, left, top, bottom, front, back（Three BoxGeometry）
 */
function setSkinUVs(
  box: BoxGeometry,
  u: number,
  v: number,
  width: number,
  height: number,
  depth: number,
  textureWidth: number,
  textureHeight: number,
): void {
  const toFace = (x1: number, y1: number, x2: number, y2: number) => [
    new Vector2(x1 / textureWidth, 1 - y2 / textureHeight),
    new Vector2(x2 / textureWidth, 1 - y2 / textureHeight),
    new Vector2(x2 / textureWidth, 1 - y1 / textureHeight),
    new Vector2(x1 / textureWidth, 1 - y1 / textureHeight),
  ];

  const top = toFace(u + depth, v, u + width + depth, v + depth);
  const bottom = toFace(
    u + width + depth,
    v,
    u + width * 2 + depth,
    v + depth,
  );
  const left = toFace(u, v + depth, u + depth, v + depth + height);
  const front = toFace(
    u + depth,
    v + depth,
    u + width + depth,
    v + depth + height,
  );
  const right = toFace(
    u + width + depth,
    v + depth,
    u + width + depth * 2,
    v + height + depth,
  );
  const back = toFace(
    u + width + depth * 2,
    v + depth,
    u + width * 2 + depth * 2,
    v + height + depth,
  );

  // BoxGeometry groups: +x -x +y -y +z -z → right left top bottom front back
  const uvRight = [right[3], right[2], right[0], right[1]];
  const uvLeft = [left[3], left[2], left[0], left[1]];
  const uvTop = [top[3], top[2], top[0], top[1]];
  const uvBottom = [bottom[0], bottom[1], bottom[3], bottom[2]];
  const uvFront = [front[3], front[2], front[0], front[1]];
  const uvBack = [back[3], back[2], back[0], back[1]];

  const data: number[] = [];
  for (const face of [uvRight, uvLeft, uvTop, uvBottom, uvFront, uvBack]) {
    for (const uv of face) {
      data.push(uv.x, uv.y);
    }
  }
  const attr = box.attributes.uv as BufferAttribute;
  attr.set(new Float32Array(data));
  attr.needsUpdate = true;
}
