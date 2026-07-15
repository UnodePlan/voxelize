/**
 * Veloren 风格人形：.vox 零件拼装 + 简易走/挖动画。
 * 资产来自 Veloren（GPL-3.0），见 assets/single/veloren/。
 */

import { Group, MathUtils, Quaternion, Vector3 } from "three";

import chestUrl from "../assets/single/veloren/figure/body/chest_male.vox?url";
import footUrl from "../assets/single/veloren/figure/body/foot.vox?url";
import handUrl from "../assets/single/veloren/figure/body/hand.vox?url";
import pantsUrl from "../assets/single/veloren/figure/body/pants_male.vox?url";
import eyesUrl from "../assets/single/veloren/figure/eyes/general/male_default-0.vox?url";
import hairUrl from "../assets/single/veloren/figure/hair/human/male-0.vox?url";
import headUrl from "../assets/single/veloren/figure/head/human/male.vox?url";

import type { MannequinActor } from "./mannequin-actor";
import { loadVoxUrl } from "./vox-loader";
import { disposeObjectMesh, voxModelToMesh } from "./vox-mesh";

export type { MannequinActor };

const VOXEL = 0.09;

/**
 * 异步构建 Veloren 人形假人。
 */
export async function createVelorenCharacter(
  username = "Veloren预览",
): Promise<VelorenCharacter> {
  const [chest, pants, hand, foot, head, hair, eyes] = await Promise.all([
    loadVoxUrl(chestUrl),
    loadVoxUrl(pantsUrl),
    loadVoxUrl(handUrl),
    loadVoxUrl(footUrl),
    loadVoxUrl(headUrl),
    loadVoxUrl(hairUrl),
    loadVoxUrl(eyesUrl),
  ]);

  const character = new VelorenCharacter(username);
  character.buildFromParts({
    chest: voxModelToMesh(chest, { voxelSize: VOXEL }),
    pants: voxModelToMesh(pants, { voxelSize: VOXEL }),
    leftHand: voxModelToMesh(hand, { voxelSize: VOXEL }),
    rightHand: voxModelToMesh(hand, { voxelSize: VOXEL, mirrorX: true }),
    leftFoot: voxModelToMesh(foot, { voxelSize: VOXEL }),
    rightFoot: voxModelToMesh(foot, { voxelSize: VOXEL, mirrorX: true }),
    head: voxModelToMesh(head, { voxelSize: VOXEL }),
    hair: voxModelToMesh(hair, { voxelSize: VOXEL }),
    eyes: voxModelToMesh(eyes, { voxelSize: VOXEL }),
  });
  return character;
}

interface PartMeshes {
  chest: import("three").Mesh;
  pants: import("three").Mesh;
  leftHand: import("three").Mesh;
  rightHand: import("three").Mesh;
  leftFoot: import("three").Mesh;
  rightFoot: import("three").Mesh;
  head: import("three").Mesh;
  hair: import("three").Mesh;
  eyes: import("three").Mesh;
}

/**
 * 组层级（Y-up，原点在眼睛高度附近，兼容现有 mannequin 放置逻辑）：
 * root(eye)
 *   bodyRoot
 *     torso → chest
 *     headGroup → head + hair + eyes
 *     leftArmPivot → leftHand
 *     rightArmPivot → rightHand
 *     leftLegPivot → leftFoot + 半截 pants 视觉靠脚
 *     rightLegPivot → rightFoot
 *     hips → pants
 */
export class VelorenCharacter implements MannequinActor {
  readonly root = new Group();
  private readonly bodyRoot = new Group();
  private readonly headGroup = new Group();
  private readonly leftArmPivot = new Group();
  private readonly rightArmPivot = new Group();
  private readonly leftLegPivot = new Group();
  private readonly rightLegPivot = new Group();
  private readonly hips = new Group();
  private readonly torso = new Group();

  private readonly targetPos = new Vector3();
  private readonly targetBodyQuat = new Quaternion();
  private readonly targetHeadQuat = new Quaternion();
  private readonly scratchDir = new Vector3();
  private readonly yawAxis = new Vector3(0, 1, 0);

  private speed = 0;
  private digSwingT = 0;
  private digActive = false;
  eyeHeight = 1.6;
  private built = false;

  constructor(private readonly label: string) {
    this.root.name = `veloren-humanoid:${label}`;
    this.root.add(this.bodyRoot);
    this.bodyRoot.add(
      this.hips,
      this.torso,
      this.headGroup,
      this.leftArmPivot,
      this.rightArmPivot,
      this.leftLegPivot,
      this.rightLegPivot,
    );
  }

  get username(): string {
    return this.label;
  }

  buildFromParts(parts: PartMeshes): void {
    if (this.built) return;
    this.built = true;

    const sizeOf = (m: import("three").Mesh) =>
      (m.userData.size as { x: number; y: number; z: number } | undefined) ?? {
        x: 0.4,
        y: 0.4,
        z: 0.4,
      };

    const chestS = sizeOf(parts.chest);
    const pantsS = sizeOf(parts.pants);
    const headS = sizeOf(parts.head);
    const handS = sizeOf(parts.leftHand);
    const footS = sizeOf(parts.leftFoot);

    // 自下而上：脚 → 裤 → 胸 → 头
    const pantsY = footS.y + pantsS.y / 2;
    const chestY = footS.y + pantsS.y * 0.55 + chestS.y / 2;
    const headY = footS.y + pantsS.y * 0.55 + chestS.y + headS.y / 2 - 0.02;
    const shoulderY = footS.y + pantsS.y * 0.55 + chestS.y * 0.75;
    const hipY = footS.y + pantsS.y * 0.15;

    this.eyeHeight = headY; // 眼大约在头心；放置时用脚底+eyeHeight

    // 原点提到眼高：整棵子树下移 eyeHeight
    const lift = -this.eyeHeight;

    this.hips.position.set(0, pantsY + lift, 0);
    this.hips.add(parts.pants);

    this.torso.position.set(0, chestY + lift, 0);
    this.torso.add(parts.chest);

    this.headGroup.position.set(0, headY + lift, 0);
    this.headGroup.add(parts.head);
    // 头发/眼睛叠在头上（局部小偏移）
    parts.hair.position.set(0, headS.y * 0.15, 0);
    parts.eyes.position.set(0, headS.y * 0.05, headS.z * 0.15);
    this.headGroup.add(parts.hair, parts.eyes);

    const armX = chestS.x * 0.55 + handS.x * 0.2;
    this.leftArmPivot.position.set(-armX, shoulderY + lift, 0);
    this.rightArmPivot.position.set(armX, shoulderY + lift, 0);
    parts.leftHand.position.set(0, -handS.y * 0.35, 0);
    parts.rightHand.position.set(0, -handS.y * 0.35, 0);
    this.leftArmPivot.add(parts.leftHand);
    this.rightArmPivot.add(parts.rightHand);

    const legX = pantsS.x * 0.22;
    this.leftLegPivot.position.set(-legX, hipY + lift, 0);
    this.rightLegPivot.position.set(legX, hipY + lift, 0);
    parts.leftFoot.position.set(0, -footS.y * 0.1, 0);
    parts.rightFoot.position.set(0, -footS.y * 0.1, 0);
    this.leftLegPivot.add(parts.leftFoot);
    this.rightLegPivot.add(parts.rightFoot);
  }

  set(position: number[], direction: number[]): void {
    this.targetPos.set(position[0], position[1], position[2]);
    const dx = direction[0];
    const dz = direction[2];
    if (Math.abs(dx) + Math.abs(dz) > 1e-5) {
      this.scratchDir.set(dx, 0, dz).normalize();
      // 面朝 -Z 为前时，用 look direction 构建 body 四元数
      const yaw = Math.atan2(this.scratchDir.x, this.scratchDir.z);
      this.targetBodyQuat.setFromAxisAngle(this.yawAxis, yaw);
      this.targetHeadQuat.copy(this.targetBodyQuat);
    }

    // 速度：位移幅度
    const dist = this.root.position.distanceTo(this.targetPos);
    this.speed = dist > 0.0005 ? 1.4 : 0;
  }

  snapToTarget(): void {
    this.root.position.copy(this.targetPos);
    this.bodyRoot.quaternion.copy(this.targetBodyQuat);
    this.headGroup.quaternion.copy(this.targetHeadQuat);
  }

  playArmSwingAnimation(): void {
    this.digActive = true;
    this.digSwingT = 0;
  }

  update(): void {
    // 位姿插值
    this.root.position.lerp(this.targetPos, 0.35);
    this.bodyRoot.quaternion.slerp(this.targetBodyQuat, 0.2);
    this.headGroup.quaternion.slerp(this.targetHeadQuat, 0.25);

    const t = performance.now() / 1000;
    const walk = this.speed > 0.01;
    const amp = walk ? 0.9 : 0.06;
    const freq = walk ? 7 : 1.2;

    // 腿
    this.leftLegPivot.rotation.x = Math.sin(t * freq) * amp * 0.7;
    this.rightLegPivot.rotation.x = Math.sin(t * freq + Math.PI) * amp * 0.7;

    // 臂：走路时对侧摆；挖掘时右臂大力挥
    if (this.digActive) {
      this.digSwingT += 0.08;
      const swing = Math.sin(this.digSwingT * Math.PI) * 1.4;
      this.rightArmPivot.rotation.x = -0.4 - swing;
      this.leftArmPivot.rotation.x = MathUtils.lerp(
        this.leftArmPivot.rotation.x,
        0.15,
        0.2,
      );
      if (this.digSwingT >= 1) {
        this.digActive = false;
        this.digSwingT = 0;
      }
    } else {
      this.leftArmPivot.rotation.x = Math.sin(t * freq + Math.PI) * amp * 0.85;
      this.rightArmPivot.rotation.x = Math.sin(t * freq) * amp * 0.85;
    }

    // 轻微躯干晃动
    this.torso.rotation.y = Math.sin(t * freq * 0.5) * (walk ? 0.08 : 0.02);
  }

  dispose(): void {
    disposeObjectMesh(this.root);
  }
}
