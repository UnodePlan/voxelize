import { protocol } from "@voxelize/protocol";
import { zlibSync } from "fflate";
import { describe, expect, it } from "vitest";

import { decodeMessage } from "./decode-utils";

describe("decodeMessage", () => {
  it("decodes a zlib-compressed INIT and parses its JSON payload", () => {
    const encoded = protocol.Message.encode(
      protocol.Message.create({
        type: protocol.Message.Type.INIT,
        json: JSON.stringify({ id: "player-1", world: "match:v1:test" }),
      }),
    ).finish();

    expect(decodeMessage(zlibSync(encoded), [])).toMatchObject({
      type: "INIT",
      json: { id: "player-1", world: "match:v1:test" },
    });
  });
});
