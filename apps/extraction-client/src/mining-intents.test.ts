import { describe, expect, it } from "vitest";

import {
  decodeMiningIntent,
  decodeProtocolEnvelope,
} from "../../../contracts/extraction/v1/typescript";
import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import type { ExtractionManifest } from "../../../contracts/extraction/v1/typescript";
import { MiningIntentFactory } from "./mining-intents";

const manifest = manifestJson as ExtractionManifest;

describe("MiningIntentFactory", () => {
  it("只生成严格递增的 start/maintain/cancel 意图", () => {
    const factory = new MiningIntentFactory(manifest.protocolVersion, 8);
    const messages = [factory.start([4, 28, -9]), factory.maintain(), factory.cancel()];

    expect(messages.map((message) => message.sequence)).toEqual([9, 10, 11]);
    expect(
      messages.map((message) =>
        decodeMiningIntent(decodeProtocolEnvelope(message, manifest)),
      ),
    ).toEqual([
      {
        requestId: messages[0].requestId,
        sequence: 9,
        payload: { action: "start", voxel: [4, 28, -9] },
      },
      {
        requestId: messages[1].requestId,
        sequence: 10,
        payload: { action: "maintain" },
      },
      {
        requestId: messages[2].requestId,
        sequence: 11,
        payload: { action: "cancel" },
      },
    ]);
  });

  it("sequence 耗尽时失败关闭且不回绕", () => {
    const factory = new MiningIntentFactory(
      manifest.protocolVersion,
      4_294_967_294,
    );
    expect(factory.cancel().sequence).toBe(4_294_967_295);
    expect(() => factory.maintain()).toThrow(/sequence/);
  });

  it("拒绝浮点和越界坐标且不消耗 sequence", () => {
    const factory = new MiningIntentFactory(manifest.protocolVersion);
    expect(() => factory.start([1.5, 2, 3])).toThrow(/i32/);
    expect(() => factory.start([2_147_483_648, 2, 3])).toThrow(/i32/);
    expect(factory.start([1, 2, 3]).sequence).toBe(0);
  });
});
