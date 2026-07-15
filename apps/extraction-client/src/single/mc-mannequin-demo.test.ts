import { describe, expect, it, vi } from "vitest";

import { McMannequinDemo } from "./mc-mannequin-demo";

function fakeCharacter() {
  return {
    eyeHeight: 1.75,
    position: { set: vi.fn() },
    newPosition: { set: vi.fn() },
    set: vi.fn(),
    snapToTarget: vi.fn(),
    update: vi.fn(),
    playArmSwingAnimation: vi.fn(),
    setHeldItem: vi.fn(),
  };
}

describe("McMannequinDemo", () => {
  it("starts in walk phase and moves via character.set", () => {
    const character = fakeCharacter();
    const demo = new McMannequinDemo({
      character: character as never,
      surfaceY: () => 16,
      fromXZ: [0, 0],
      toXZ: [4, 0],
      walkSpeed: 2,
      walkLegSeconds: 10,
      mineSeconds: 3,
      digIntervalSeconds: 0.5,
    });
    expect(demo.phaseName).toBe("walk");
    expect(character.set).toHaveBeenCalled();
    character.set.mockClear();
    demo.update(100); // 0.1s
    expect(character.set).toHaveBeenCalled();
    expect(character.update).toHaveBeenCalled();
  });

  it("switches to mine after walk duration and swings arm", () => {
    const character = fakeCharacter();
    const demo = new McMannequinDemo({
      character: character as never,
      surfaceY: () => 16,
      fromXZ: [0, 0],
      toXZ: [4, 0],
      walkSpeed: 2,
      walkLegSeconds: 0.5,
      mineSeconds: 2,
      digIntervalSeconds: 0.5,
    });
    // 刚好走完一程，且 mine 阶段较长，不会在同帧又切回 walk
    demo.update(500);
    expect(demo.phaseName).toBe("mine");
    expect(character.playArmSwingAnimation).toHaveBeenCalled();
  });

  it("returns to walk after mine duration", () => {
    const character = fakeCharacter();
    const demo = new McMannequinDemo({
      character: character as never,
      surfaceY: () => 16,
      fromXZ: [0, 0],
      toXZ: [4, 0],
      walkSpeed: 2,
      walkLegSeconds: 0.5,
      mineSeconds: 0.5,
      digIntervalSeconds: 0.5,
    });
    demo.update(500); // → mine
    expect(demo.phaseName).toBe("mine");
    demo.update(500); // → walk
    expect(demo.phaseName).toBe("walk");
  });

  it("equips sword while walking and pickaxe while mining", () => {
    const character = fakeCharacter();
    const sword = { name: "sword" };
    const pickaxe = { name: "pickaxe" };
    const demo = new McMannequinDemo({
      character: character as never,
      surfaceY: () => 16,
      fromXZ: [0, 0],
      toXZ: [4, 0],
      walkSpeed: 2,
      walkLegSeconds: 0.5,
      mineSeconds: 0.5,
      digIntervalSeconds: 0.5,
      heldItems: { walk: sword as never, mine: pickaxe as never },
    });
    expect(character.setHeldItem).toHaveBeenCalledWith(sword);
    character.setHeldItem.mockClear();
    demo.update(500); // → mine
    expect(demo.phaseName).toBe("mine");
    expect(character.setHeldItem).toHaveBeenCalledWith(pickaxe);
    character.setHeldItem.mockClear();
    demo.update(500); // → walk
    expect(character.setHeldItem).toHaveBeenCalledWith(sword);
  });
});
