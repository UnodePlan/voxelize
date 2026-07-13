import { describe, expect, it } from "vitest";

import extractionFixtureJson from "../../../contracts/extraction/v1/fixtures/extraction-states.json";
import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import {
  decodeEnvelopeFixture,
  decodeExtractionManifest,
  decodeExtractionStateEnvelope,
} from "../../../contracts/extraction/v1/typescript";

describe("extraction state contract", () => {
  const manifest = decodeExtractionManifest(manifestJson as unknown);
  const fixture = decodeEnvelopeFixture(extractionFixtureJson as unknown);

  it.each(fixture.cases)("matches extraction state $name", (fixtureCase) => {
    let accepted = true;
    try {
      decodeExtractionStateEnvelope(fixtureCase.value, manifest);
    } catch {
      accepted = false;
    }
    expect(accepted).toBe(fixtureCase.accept);
  });
});
