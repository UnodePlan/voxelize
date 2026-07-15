import { Mesh, MeshBasicMaterial, PlaneGeometry } from "three";
import { describe, expect, it } from "vitest";

import { applyThirdPersonHeldPose } from "./mc-held-item";

function dummyItemMesh(): Mesh {
  return new Mesh(new PlaneGeometry(8, 8), new MeshBasicMaterial());
}

describe("applyThirdPersonHeldPose", () => {
  it("flips Z by π so handle (texture bottom-left) sits toward the hand", () => {
    const sword = dummyItemMesh();
    applyThirdPersonHeldPose(sword, "sword");
    // 翻柄：rotation.z 含 +π 分量（相对未翻的对角握持）
    expect(sword.rotation.z).toBeGreaterThan(Math.PI * 0.9);
    expect(sword.rotation.order).toBe("YXZ");

    const pickaxe = dummyItemMesh();
    applyThirdPersonHeldPose(pickaxe, "pickaxe");
    expect(pickaxe.rotation.z).toBeGreaterThan(Math.PI * 0.9);
  });

  it("offsets mesh so grip is near heldSlot origin, not floating mid-blade", () => {
    const sword = dummyItemMesh();
    applyThirdPersonHeldPose(sword, "sword");
    // 中心靠近手心，不应再大幅甩到臂前（旧值 z≈-3.5）
    expect(Math.abs(sword.position.x)).toBeLessThan(2);
    expect(Math.abs(sword.position.y)).toBeLessThan(3);
    expect(Math.abs(sword.position.z)).toBeLessThan(3);
  });
});
