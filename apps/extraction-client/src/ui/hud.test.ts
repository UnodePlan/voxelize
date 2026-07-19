import { describe, expect, it } from "vitest";

import stateFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  decodeGameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import { INITIAL_APP_STATE } from "../app/state";

import { renderHud } from "./hud";

const manifest = decodeExtractionManifest(manifestJson);
const fixture = stateFixtures.cases.find(
  ({ name }) => name === "valid-alive-gameplay-state",
);
if (fixture === undefined) {
  throw new Error("missing alive gameplay fixture");
}

describe("match HUD", () => {
  it("does not invent health or inventory before the full snapshot", () => {
    const html = renderHud({
      ...INITIAL_APP_STATE,
      screen: "match",
      manifest,
      activeMatchId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      connection: "reconnecting",
    });

    expect(html).toContain("同步权威状态");
    expect(html).not.toContain('class="heart"');
    expect(html).not.toContain('class="inventory-slot"');
    expect(html).not.toContain("生命值 20/20");
  });

  it("keeps ten heart slots, one half heart and twelve inventory slots", () => {
    const gameplay = decodeGameplayStateData(fixture.value, manifest);
    const html = renderHud({
      ...INITIAL_APP_STATE,
      screen: "match",
      manifest,
      activeMatchId: gameplay.matchId,
      gameplay: {
        ...gameplay,
        health: {
          ...gameplay.health,
          data: { status: "alive", currentHalfHearts: 17, maxHalfHearts: 20 },
        },
      },
      connection: "online",
    });

    expect(html).toContain("player-hud-stack");
    expect(html).toContain("loadout-row");
    expect(html.match(/<span class="heart"/gu)).toHaveLength(10);
    expect(html.match(/data-fill="full"/gu)).toHaveLength(8);
    expect(html.match(/data-fill="half"/gu)).toHaveLength(1);
    expect(html.match(/data-fill="empty"/gu)).toHaveLength(1);
    expect(html.match(/class="inventory-slot"/gu)).toHaveLength(12);
    expect(html).not.toContain("death-banner");
  });

  it("shows death banner with lost resource summary", () => {
    const gameplay = decodeGameplayStateData(fixture.value, manifest);
    const html = renderHud({
      ...INITIAL_APP_STATE,
      screen: "match",
      manifest,
      activeMatchId: gameplay.matchId,
      gameplay: {
        ...gameplay,
        health: {
          ...gameplay.health,
          revision: 3,
          data: { status: "dead", currentHalfHearts: 0, maxHalfHearts: 20 },
        },
        deathResult: {
          protocolVersion: gameplay.health.protocolVersion,
          type: "state",
          matchId: gameplay.matchId,
          stream: "deathResult",
          revision: 3,
          data: {
            cause: "melee",
            killerPublicPlayerId: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            survivedMs: 12_000,
            mined: { dirt: 2, gold: 0, diamond: 0 },
            pickedUp: { dirt: 0, gold: 0, diamond: 0 },
            lost: { dirt: 3, gold: 1, diamond: 0 },
          },
        },
      },
      connection: "online",
    });

    expect(html).toContain("death-banner");
    expect(html).toContain("你已阵亡");
    expect(html).toContain("泥土×3");
    expect(html).toContain("黄金×1");
    expect(html).toContain("is-dead");
  });
});

describe("formatTallyLine", () => {
  it("formats non-zero resources only", async () => {
    const { formatTallyLine } = await import("./hud");
    expect(formatTallyLine({ dirt: 0, gold: 0, diamond: 0 })).toBe("");
    expect(formatTallyLine({ dirt: 2, gold: 1, diamond: 0 })).toBe(
      "泥土×2 黄金×1",
    );
  });
});
