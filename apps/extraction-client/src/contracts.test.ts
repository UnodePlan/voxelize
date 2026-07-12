import { describe, expect, it } from "vitest";

import envelopeFixtureJson from "../../../contracts/extraction/v1/fixtures/envelopes.json";
import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import {
  decodeEnvelopeFixture,
  decodeExtractionManifest,
  decodeProtocolEnvelope,
} from "../../../contracts/extraction/v1/typescript";

describe("extraction contract fixtures", () => {
  const manifest = decodeExtractionManifest(manifestJson as unknown);
  const envelopeFixture = decodeEnvelopeFixture(envelopeFixtureJson as unknown);

  it("keeps stable resources, stack limits, IDs and score weights", () => {
    expect(
      manifest.resources.map(
        ({ key, voxelId, itemId, maxStack, scoreWeight }) => ({
          key,
          voxelId,
          itemId,
          maxStack,
          scoreWeight,
        }),
      ),
    ).toEqual([
      {
        key: "dirt",
        voxelId: 1001,
        itemId: 2001,
        maxStack: 64,
        scoreWeight: 1,
      },
      {
        key: "gold",
        voxelId: 1002,
        itemId: 2002,
        maxStack: 64,
        scoreWeight: 10,
      },
      {
        key: "diamond",
        voxelId: 1003,
        itemId: 2003,
        maxStack: 64,
        scoreWeight: 100,
      },
    ]);
  });

  it("rejects incomplete manifest versions and error taxonomies", () => {
    expect(() =>
      decodeExtractionManifest({ ...manifestJson, catalogVersion: 0 }),
    ).toThrow();
    expect(() =>
      decodeExtractionManifest({ ...manifestJson, errorCodes: [] }),
    ).toThrow();
    expect(() =>
      decodeExtractionManifest({
        ...manifestJson,
        errorCodes: ["CLIENT_INVENTED_ERROR"],
      }),
    ).toThrow();
  });

  it.each(envelopeFixture.cases)("matches $name", (fixtureCase) => {
    const decode = () => decodeProtocolEnvelope(fixtureCase.value, manifest);
    if (fixtureCase.accept) {
      expect(decode).not.toThrow();
    } else {
      expect(decode).toThrow();
    }
  });
});
