import { describe, expect, it } from "vitest";

import {
  applyDamage,
  attackDamageHalfHearts,
  fullHealth,
  heartsFromHealth,
  humanoidAabb,
  knockbackImpulse,
  KNOCKBACK_HORIZONTAL,
  KNOCKBACK_UP,
  LOCAL_ATTACK_COOLDOWN_MS,
  LOCAL_ATTACK_RANGE,
  LOCAL_MAX_HEALTH,
  LOCAL_MAX_HEARTS,
  raycastAabb,
} from "./combat";

describe("combat health", () => {
  it("starts at 10 full hearts (20 half-hearts)", () => {
    expect(fullHealth()).toBe(LOCAL_MAX_HEALTH);
    expect(LOCAL_MAX_HEARTS).toBe(10);
    expect(heartsFromHealth(fullHealth())).toEqual(
      Array.from({ length: 10 }, () => "full"),
    );
  });

  it("fist deals half a heart; sword deals one heart", () => {
    expect(attackDamageHalfHearts("empty")).toBe(1);
    expect(attackDamageHalfHearts("pickaxe")).toBe(1);
    expect(attackDamageHalfHearts("sword")).toBe(2);
  });

  it("applies damage and clamps at zero", () => {
    expect(applyDamage(20, 1)).toBe(19);
    expect(applyDamage(20, 2)).toBe(18);
    expect(applyDamage(1, 5)).toBe(0);
    expect(applyDamage(0, 1)).toBe(0);
  });

  it("renders half hearts correctly", () => {
    // 19 半心 = 9 满 + 1 半
    expect(heartsFromHealth(19)).toEqual([
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "half",
    ]);
    // 1 半心 = 1 半 + 9 空
    expect(heartsFromHealth(1)[0]).toBe("half");
    expect(heartsFromHealth(1).filter((h) => h === "empty")).toHaveLength(9);
  });

  it("aligns melee range and cooldown with production", () => {
    expect(LOCAL_ATTACK_RANGE).toBe(3.0);
    expect(LOCAL_ATTACK_COOLDOWN_MS).toBe(600);
  });
});

describe("combat knockback", () => {
  it("pushes along look xz; sword stronger than fist", () => {
    const fist = knockbackImpulse("empty", [0, 0, -1]);
    const sword = knockbackImpulse("sword", [0, 0, -1]);
    expect(fist[0]).toBeCloseTo(0, 5);
    expect(fist[2]).toBeCloseTo(-KNOCKBACK_HORIZONTAL.empty, 5);
    expect(fist[1]).toBeCloseTo(KNOCKBACK_UP, 5);
    expect(sword[2]).toBeCloseTo(-KNOCKBACK_HORIZONTAL.sword, 5);
    expect(Math.abs(sword[2])).toBeGreaterThan(Math.abs(fist[2]));
  });

  it("normalizes diagonal look on the horizontal plane", () => {
    const imp = knockbackImpulse("pickaxe", [1, 0.5, 1]);
    const hLen = Math.hypot(imp[0], imp[2]);
    expect(hLen).toBeCloseTo(KNOCKBACK_HORIZONTAL.pickaxe, 5);
    expect(imp[0]).toBeCloseTo(imp[2], 5);
    expect(imp[1]).toBeCloseTo(KNOCKBACK_UP, 5);
  });

  it("falls back to -Z when look is pure pitch", () => {
    const imp = knockbackImpulse("empty", [0, -1, 0]);
    expect(imp[0]).toBeCloseTo(0, 5);
    expect(imp[2]).toBeCloseTo(-KNOCKBACK_HORIZONTAL.empty, 5);
  });
});

describe("combat raycast", () => {
  it("hits a box in front of the camera", () => {
    const box = humanoidAabb([0, 1.75, -2], 1.75);
    const hit = raycastAabb([0, 1.6, 0], [0, 0, -1], box, 4);
    expect(hit).not.toBeNull();
    expect(hit!).toBeGreaterThan(0);
    expect(hit!).toBeLessThan(3.5);
  });

  it("misses when looking away", () => {
    const box = humanoidAabb([0, 1.75, -2], 1.75);
    const hit = raycastAabb([0, 1.6, 0], [0, 0, 1], box, 4);
    expect(hit).toBeNull();
  });
});
