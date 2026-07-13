import { describe, expect, it } from "vitest";

import envelopeFixtureJson from "../../../contracts/extraction/v1/fixtures/envelopes.json";
import gameplayIntentFixtureJson from "../../../contracts/extraction/v1/fixtures/gameplay-intents.json";
import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import miningStateFixtureJson from "../../../contracts/extraction/v1/fixtures/mining-states.json";
import {
  decodeEnvelopeFixture,
  decodeDropSlotIntent,
  decodeExtractionManifest,
  decodeMiningIntent,
  decodeMiningStateEnvelope,
  decodeProtocolEnvelope,
} from "../../../contracts/extraction/v1/typescript";

describe("extraction contract fixtures", () => {
  const manifest = decodeExtractionManifest(manifestJson as unknown);
  const envelopeFixture = decodeEnvelopeFixture(envelopeFixtureJson as unknown);
  const gameplayIntentFixture = decodeEnvelopeFixture(
    gameplayIntentFixtureJson as unknown,
  );
  const miningStateFixture = decodeEnvelopeFixture(
    miningStateFixtureJson as unknown,
  );

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
    expect(manifest.equipment).toEqual([
      { key: "basic_pickaxe", itemId: 2101 },
      { key: "basic_melee_weapon", itemId: 2102 },
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
    expect(() =>
      decodeExtractionManifest({
        ...manifestJson,
        equipment: [
          { ...manifestJson.equipment[0], itemId: 2_147_483_648 },
          manifestJson.equipment[1],
        ],
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

  it.each(gameplayIntentFixture.cases)("matches gameplay $name", (fixtureCase) => {
    const decode = () => {
      const envelope = decodeProtocolEnvelope(fixtureCase.value, manifest);
      if (fixtureCase.route === "pvp:v1:drop-slot") {
        return decodeDropSlotIntent(envelope);
      }
      if (fixtureCase.route === "pvp:v1:mining") {
        return decodeMiningIntent(envelope);
      }
      throw new Error(`unknown gameplay fixture route ${fixtureCase.route}`);
    };
    if (fixtureCase.accept) {
      expect(decode).not.toThrow();
    } else {
      expect(decode).toThrow();
    }
  });

  it.each(miningStateFixture.cases)("matches mining state $name", (fixtureCase) => {
    const decode = () => decodeMiningStateEnvelope(fixtureCase.value, manifest);
    if (fixtureCase.accept) {
      expect(decode).not.toThrow();
    } else {
      expect(decode).toThrow();
    }
  });
});
