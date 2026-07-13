import { describe, expect, it } from "vitest";

import stateFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  decodeGameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

import { hasTerminalGameplay } from "./match-coordinator-support";

const manifest = decodeExtractionManifest(manifestJson);

describe("match coordinator support", () => {
  it("treats a closed extraction window as a result-polling boundary", () => {
    const deadFixture = stateFixtures.cases.find(
      ({ name }) => name === "valid-dead-gameplay-state",
    );
    const state = decodeGameplayStateData(deadFixture?.value, manifest);

    expect(state.extraction.data.status).toBe("closed");
    expect(hasTerminalGameplay({ ...state, deathResult: null })).toBe(true);
  });
});
