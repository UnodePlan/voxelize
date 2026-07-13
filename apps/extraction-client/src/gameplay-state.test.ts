import { describe, expect, it } from "vitest";

import getStateFixtureJson from "../../../contracts/extraction/v1/fixtures/get-state-results.json";
import inventoryStateFixtureJson from "../../../contracts/extraction/v1/fixtures/inventory-states.json";
import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import {
  decodeEnvelopeFixture,
  decodeExtractionManifest,
  decodeGameplayStateData,
  decodeInventoryStateEnvelope,
} from "../../../contracts/extraction/v1/typescript";

import {
  INITIAL_GAMEPLAY_VIEW_STATE,
  reduceGameplayState,
  reduceInventoryState,
} from "./gameplay-state";

const MATCH_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

describe("gameplay state reducer", () => {
  const manifest = decodeExtractionManifest(manifestJson as unknown);
  const fixture = decodeEnvelopeFixture(getStateFixtureJson as unknown);
  const inventoryFixture = decodeEnvelopeFixture(
    inventoryStateFixtureJson as unknown,
  );
  const alive = decodeGameplayStateData(fixture.cases[0].value, manifest);
  const dead = decodeGameplayStateData(fixture.cases[1].value, manifest);
  const inventory = decodeInventoryStateEnvelope(
    inventoryFixture.cases[0].value,
    manifest,
  );

  it("仅接收当前比赛的完整快照", () => {
    const state = reduceGameplayState(
      INITIAL_GAMEPLAY_VIEW_STATE,
      alive,
      MATCH_ID,
    );

    expect(state.snapshot).toBe(alive);
    expect(state.snapshot?.inventory.lastDropSequence).toBe(17);
    expect(
      reduceGameplayState(state, alive, "cccccccc-cccc-4ccc-8ccc-cccccccccccc"),
    ).toBe(state);
  });

  it("拒绝权威丢弃游标旁的未知字段", () => {
    const invalid = structuredClone(fixture.cases[0].value) as {
      inventory: Record<string, unknown>;
    };
    invalid.inventory.clientDropSequence = 18;

    expect(() => decodeGameplayStateData(invalid, manifest)).toThrow(
      "unknown field clientDropSequence",
    );
  });

  it("原子替换更新且保留死亡终态", () => {
    const aliveState = reduceGameplayState(
      INITIAL_GAMEPLAY_VIEW_STATE,
      alive,
      MATCH_ID,
    );
    const deadState = reduceGameplayState(aliveState, dead, MATCH_ID);

    expect(deadState.snapshot?.health.data.status).toBe("dead");
    expect(deadState.snapshot?.inventory.frozen).toBe(true);
    expect(deadState.snapshot?.deathResult).not.toBeNull();
  });

  it("任一子流 revision 回退时忽略整份快照", () => {
    const current = reduceGameplayState(
      INITIAL_GAMEPLAY_VIEW_STATE,
      dead,
      MATCH_ID,
    );
    const stale = {
      ...dead,
      inventory: { ...dead.inventory, revision: dead.inventory.revision - 1 },
    };

    expect(reduceGameplayState(current, stale, MATCH_ID)).toBe(current);
  });

  it("死亡后不接受缺少 deathResult 的快照", () => {
    const current = reduceGameplayState(
      INITIAL_GAMEPLAY_VIEW_STATE,
      dead,
      MATCH_ID,
    );

    expect(reduceGameplayState(current, alive, MATCH_ID)).toBe(current);
  });

  it("inventory 增量拒绝跨局、重复和旧 revision", () => {
    const current = reduceGameplayState(
      INITIAL_GAMEPLAY_VIEW_STATE,
      alive,
      MATCH_ID,
    );
    const otherMatch = {
      ...inventory,
      matchId: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
    };

    expect(reduceInventoryState(current, otherMatch, MATCH_ID)).toBe(current);

    const updated = reduceInventoryState(current, inventory, MATCH_ID);
    expect(updated.snapshot?.inventory.revision).toBe(5);
    expect(reduceInventoryState(updated, inventory, MATCH_ID)).toBe(updated);

    const stale = {
      ...inventory,
      revision: 4,
      data: {
        ...inventory.data,
        inventory: { ...inventory.data.inventory, revision: 4 },
      },
    };
    expect(reduceInventoryState(updated, stale, MATCH_ID)).toBe(updated);
  });
});
