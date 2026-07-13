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

    expect(html.match(/<span class="heart"/gu)).toHaveLength(10);
    expect(html.match(/data-fill="full"/gu)).toHaveLength(8);
    expect(html.match(/data-fill="half"/gu)).toHaveLength(1);
    expect(html.match(/data-fill="empty"/gu)).toHaveLength(1);
    expect(html.match(/class="inventory-slot"/gu)).toHaveLength(12);
  });
});
