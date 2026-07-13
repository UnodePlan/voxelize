import { describe, expect, it } from "vitest";

import { moveTogether } from "./live/gameplay-actions";
import type { GameplayProtocolDriver } from "./live/gameplay-driver";
import type { LiveVector3 } from "./live/gameplay-state";
import type { GameplayTime } from "./live/gameplay-time";
import type { LiveMovementInput } from "./live/wire";

describe("live movement recovery", () => {
  it("requests a jump after authoritative movement stagnates", async () => {
    const left = new StuckUntilJumpDriver([0, 50, 0], [1, 50, 0]);
    const right = new StuckUntilJumpDriver([0, 50, 2], [1, 50, 2]);
    const clock: GameplayTime = {
      async elapse() {
        return { monotonicMs: 0 };
      },
    };

    await moveTogether(
      [
        { driver: asGameplayDriver(left), target: left.target },
        { driver: asGameplayDriver(right), target: right.target },
      ],
      clock,
    );

    expect(left.jumps.slice(0, 2)).toEqual([false, true]);
    expect(right.jumps.slice(0, 2)).toEqual([false, true]);
  });
});

class StuckUntilJumpDriver {
  readonly jumps: boolean[] = [];

  constructor(
    private current: LiveVector3,
    readonly target: LiveVector3,
  ) {}

  position(): LiveVector3 {
    return this.current;
  }

  async sendMovement(input: LiveMovementInput): Promise<void> {
    this.jumps.push(input.movement.jump);
    if (input.movement.jump) this.current = this.target;
  }
}

function asGameplayDriver(
  driver: StuckUntilJumpDriver,
): GameplayProtocolDriver {
  return driver as unknown as GameplayProtocolDriver;
}
