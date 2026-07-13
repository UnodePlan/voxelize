import { describe, expect, it } from "vitest";

import {
  runExactTenCapacityScenario,
  type CapacityActor,
  type CapacityActorKind,
  type CapacityAdmission,
  type CapacityJoin,
} from "./capacity-scenario";

const MATCH_ID = "00000000-0000-4000-8000-000000000011";
const WORLD_NAME = "match-00000000000040008000000000000011";

class ScriptedCapacityActor implements CapacityActor {
  readonly actorId: string;
  readonly kind: CapacityActorKind;
  disconnected = false;

  constructor(
    index: number,
    kind: CapacityActorKind,
    private readonly rejectAdmission: boolean,
    private readonly rejectDisconnect = false,
  ) {
    this.actorId = `actor-${index}`;
    this.kind = kind;
  }

  async connect(): Promise<void> {}

  async enqueue(): Promise<CapacityAdmission> {
    return this.rejectAdmission
      ? { status: "rejected", code: "MATCH_FULL" }
      : { status: "accepted", matchId: MATCH_ID, worldName: WORLD_NAME };
  }

  async join(matchId: string, worldName: string): Promise<CapacityJoin> {
    if (this.rejectAdmission) {
      return { status: "rejected", code: "MATCH_ROSTER_LOCKED" };
    }
    return { status: "joined", matchId, worldName };
  }

  async disconnect(): Promise<void> {
    this.disconnected = true;
    if (this.rejectDisconnect) {
      throw new Error("scripted disconnect failure");
    }
  }
}

class DelayedConnectActor extends ScriptedCapacityActor {
  connected = false;

  constructor(
    index: number,
    private readonly delayMs: number,
    private readonly failConnect: boolean,
  ) {
    super(index, "protocol", index === 10);
  }

  override async connect(): Promise<void> {
    await new Promise((resolvePromise) =>
      setTimeout(resolvePromise, this.delayMs),
    );
    this.connected = true;
    if (this.failConnect) throw new Error("scripted connect failure");
  }
}

describe("capacity scenario orchestrator with a scripted driver", () => {
  it("validates the planned two-browser/eight-protocol topology without claiming a network gate", async () => {
    const actors = Array.from(
      { length: 11 },
      (_, index) =>
        new ScriptedCapacityActor(
          index,
          index < 2 ? "browser" : "protocol",
          index === 10,
        ),
    );

    const result = await runExactTenCapacityScenario(actors);

    expect(result).toEqual({
      matchId: MATCH_ID,
      worldName: WORLD_NAME,
      admittedActorIds: actors.slice(0, 10).map(({ actorId }) => actorId),
      rejectedActorId: actors[10].actorId,
      browserActors: 2,
      protocolActors: 8,
    });
    expect(actors.every(({ disconnected }) => disconnected)).toBe(true);
  });

  it("fails closed and still disconnects every actor when capacity results are skewed", async () => {
    const actors = Array.from(
      { length: 11 },
      (_, index) => new ScriptedCapacityActor(index, "protocol", false),
    );

    await expect(runExactTenCapacityScenario(actors)).rejects.toThrow(
      "expected 10 accepted actors, received 11",
    );
    expect(actors.every(({ disconnected }) => disconnected)).toBe(true);
  });

  it("fails the gate when any actor cannot be disconnected", async () => {
    const actors = Array.from(
      { length: 11 },
      (_, index) =>
        new ScriptedCapacityActor(index, "protocol", index === 10, index === 5),
    );

    await expect(runExactTenCapacityScenario(actors)).rejects.toThrow(
      "one or more actors failed to disconnect",
    );
    expect(actors.every(({ disconnected }) => disconnected)).toBe(true);
  });

  it("still disconnects every actor when the pre-cleanup hook fails", async () => {
    const actors = Array.from(
      { length: 11 },
      (_, index) =>
        new ScriptedCapacityActor(
          index,
          index < 2 ? "browser" : "protocol",
          index === 10,
        ),
    );
    let hookCalled = false;

    await expect(
      runExactTenCapacityScenario(actors, {
        beforeDisconnect: async (result) => {
          hookCalled = true;
          expect(result.admittedActorIds).toHaveLength(10);
          expect(actors.some(({ disconnected }) => disconnected)).toBe(false);
          throw new Error("visual gate failed");
        },
      }),
    ).rejects.toThrow("visual gate failed");

    expect(hookCalled).toBe(true);
    expect(actors.every(({ disconnected }) => disconnected)).toBe(true);
  });

  it("waits for every concurrent connect before cleanup after one fails", async () => {
    const actors = Array.from(
      { length: 11 },
      (_, index) =>
        new DelayedConnectActor(index, index === 10 ? 20 : 0, index === 0),
    );

    await expect(runExactTenCapacityScenario(actors)).rejects.toThrow(
      "capacity scenario connect failed: scripted connect failure",
    );

    expect(actors.every(({ connected }) => connected)).toBe(true);
    expect(actors.every(({ disconnected }) => disconnected)).toBe(true);
  });
});
