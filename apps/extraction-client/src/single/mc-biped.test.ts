import {
  DataTexture,
  Group,
  RGBAFormat,
  UnsignedByteType,
  Vector3,
} from "three";
import { describe, expect, it } from "vitest";

import { McBipedMannequin, yawFromLookXZ } from "./mc-biped";

function dummySkinTexture(size = 64): DataTexture {
  const data = new Uint8Array(size * size * 4);
  data.fill(255);
  const texture = new DataTexture(data, size, size, RGBAFormat, UnsignedByteType);
  texture.needsUpdate = true;
  (
    texture as DataTexture & { __skinW?: number; __skinH?: number }
  ).__skinW = size;
  (
    texture as DataTexture & { __skinW?: number; __skinH?: number }
  ).__skinH = size;
  return texture;
}

describe("McBipedMannequin", () => {
  it("eyeHeight matches ModelBiped feet→eye (~28px)", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    expect(mannequin.eyeHeight).toBeCloseTo(28 / 16, 5);
    mannequin.dispose();
  });

  it("walk movement advances limb swing without throwing", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    mannequin.set([0, 1.75, 0], [0, 0, 1]);
    mannequin.snapToTarget();
    mannequin.set([1, 1.75, 0], [1, 0, 0]);
    for (let i = 0; i < 12; i += 1) {
      mannequin.update();
    }
    expect(mannequin.root.position.x).toBeGreaterThan(0);
    mannequin.dispose();
  });

  it("playArmSwingAnimation completes a dig cycle", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    mannequin.set([0, 1.75, 0], [0, 0, 1]);
    mannequin.snapToTarget();
    mannequin.playArmSwingAnimation();
    for (let i = 0; i < 40; i += 1) {
      mannequin.update();
    }
    // 挖完后应回到 idle（再 update 也不抛）
    mannequin.update();
    mannequin.dispose();
  });

  it("first-person self view hides head and shifts body backward", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    mannequin.enableFirstPersonSelfView();
    // model children: head, body, rightArm, leftArm, rightLeg, leftLeg
    const model = mannequin.root.children[0] as Group;
    expect(model).toBeDefined();
    const parts = model.children;
    expect(parts[0]!.visible).toBe(false); // head 始终隐藏
    // 后移：局部 +Z（模型后向，眼后）
    expect(model.position.z).toBeGreaterThan(0);
    expect(model.position.z).toBeCloseTo(6 / 16, 5);
    mannequin.dispose();
  });

  it("setHeldItem attaches and clears right-arm held slot", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    const model = mannequin.root.children[0] as Group;
    const rightArm = model.children[2] as Group;
    const heldSlot = rightArm.children.find((c) => c.name === "mc-held-slot");
    expect(heldSlot).toBeDefined();
    const item = new Group();
    item.name = "test-sword";
    mannequin.setHeldItem(item);
    expect(heldSlot!.children).toHaveLength(1);
    expect(heldSlot!.children[0]!.name).toBe("test-sword");
    mannequin.setHeldItem(null);
    expect(heldSlot!.children).toHaveLength(0);
    mannequin.dispose();
  });

  it("syncEyeFrame shows legs/torso by look pitch, never body arms", () => {
    const mannequin = new McBipedMannequin(dummySkinTexture(), false);
    mannequin.enableFirstPersonSelfView();
    const model = mannequin.root.children[0]!;
    const body = model.children[1]!;
    const rightArm = model.children[2]!;
    const leftArm = model.children[3]!;
    const rightLeg = model.children[4]!;
    const leftLeg = model.children[5]!;

    const eye = new Vector3(10, 20, 30);
    // 平视：不显示
    mannequin.syncEyeFrame(eye, new Vector3(0, 0, -1), true);
    expect(mannequin.root.position.x).toBeCloseTo(10, 5);
    expect(mannequin.root.rotation.y).toBeCloseTo(yawFromLookXZ(0, -1), 5);
    expect(body.visible).toBe(false);
    expect(leftArm.visible).toBe(false);
    expect(rightArm.visible).toBe(false);
    expect(rightLeg.visible).toBe(false);
    expect(leftLeg.visible).toBe(false);

    // 中等低头：躯干 + 腿；身体手臂始终关（viewmodel 负责手）
    mannequin.syncEyeFrame(eye, new Vector3(0, -0.4, -0.9).normalize(), false);
    expect(body.visible).toBe(true);
    expect(leftArm.visible).toBe(false);
    expect(rightArm.visible).toBe(false);
    expect(rightLeg.visible).toBe(true);
    expect(leftLeg.visible).toBe(true);

    // 极低头：藏躯干，只留腿
    mannequin.syncEyeFrame(eye, new Vector3(0, -0.95, -0.1).normalize(), false);
    expect(body.visible).toBe(false);
    expect(leftArm.visible).toBe(false);
    expect(rightArm.visible).toBe(false);
    expect(rightLeg.visible).toBe(true);
    expect(leftLeg.visible).toBe(true);
    mannequin.dispose();
  });
});

describe("yawFromLookXZ", () => {
  it("faces local -Z along look direction", () => {
    // 朝 -Z：yaw=0 → 局部 -Z 仍为世界 -Z
    expect(yawFromLookXZ(0, -1)).toBeCloseTo(0, 5);
    // 朝 +X：yaw = -π/2
    expect(yawFromLookXZ(1, 0)).toBeCloseTo(-Math.PI / 2, 5);
  });
});
