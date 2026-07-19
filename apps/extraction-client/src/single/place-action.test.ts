import { describe, expect, it, vi } from "vitest";

import { attemptPlace, LOCAL_PLACE_COOLDOWN_MS } from "./place-action";
import type { LocalInventorySlot } from "./state";

function inv(
  entries: Array<LocalInventorySlot | null>,
): Array<LocalInventorySlot | null> {
  const base: Array<LocalInventorySlot | null> = Array.from(
    { length: 12 },
    () => null,
  );
  for (let i = 0; i < entries.length; i += 1) base[i] = entries[i];
  return base;
}

describe("attemptPlace", () => {
  it("places one dirt on rising edge and consumes inventory", () => {
    const canPlace = vi.fn(() => true);
    const place = vi.fn(() => true);
    const inventory = inv([
      null,
      null,
      null,
      { resource: "dirt", quantity: 3 },
    ]);
    const result = attemptPlace({
      pointerLocked: true,
      secondaryHeld: true,
      wasSecondaryHeld: false,
      now: 1000,
      placeCooldownUntil: 0,
      selectedSlot: 3,
      inventory,
      potential: [1, 2, 3],
      canPlace,
      place,
    });
    expect(result.kind).toBe("placed");
    if (result.kind !== "placed") return;
    expect(result.resource).toBe("dirt");
    expect(result.inventory[3]).toEqual({ resource: "dirt", quantity: 2 });
    expect(result.nextCooldownUntil).toBe(1000 + LOCAL_PLACE_COOLDOWN_MS);
    expect(place).toHaveBeenCalledWith([1, 2, 3], expect.any(Number));
  });

  it("ignores tool slots and empty potential", () => {
    const place = vi.fn(() => true);
    expect(
      attemptPlace({
        pointerLocked: true,
        secondaryHeld: true,
        wasSecondaryHeld: false,
        now: 0,
        placeCooldownUntil: 0,
        selectedSlot: 1,
        inventory: inv([]),
        potential: [0, 1, 0],
        canPlace: () => true,
        place,
      }).kind,
    ).toBe("noop");
    expect(
      attemptPlace({
        pointerLocked: true,
        secondaryHeld: true,
        wasSecondaryHeld: false,
        now: 0,
        placeCooldownUntil: 0,
        selectedSlot: 3,
        inventory: inv([{ resource: "dirt", quantity: 1 }]),
        potential: null,
        canPlace: () => true,
        place,
      }).kind,
    ).toBe("noop");
    expect(place).not.toHaveBeenCalled();
  });

  it("respects hold cooldown without rising edge", () => {
    const place = vi.fn(() => true);
    const result = attemptPlace({
      pointerLocked: true,
      secondaryHeld: true,
      wasSecondaryHeld: true,
      now: 100,
      placeCooldownUntil: 200,
      selectedSlot: 3,
      inventory: inv([null, null, null, { resource: "stone", quantity: 2 }]),
      potential: [0, 1, 0],
      canPlace: () => true,
      place,
    });
    expect(result.kind).toBe("noop");
    expect(place).not.toHaveBeenCalled();
  });
});
