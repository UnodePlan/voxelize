import { describe, expect, it } from "vitest";

import { resolveInitClientId } from "./client-id";

describe("resolveInitClientId", () => {
  it("keeps the initial and rebind identity contract", () => {
    expect(resolveInitClientId("", "player-1", false)).toBe("player-1");
    expect(resolveInitClientId("player-1", "player-1", false)).toBe("player-1");
    expect(() => resolveInitClientId("player-1", "forged", false)).toThrow(
      "Something went wrong with IDs",
    );
  });

  it("accepts a new public player id after an explicit leave", () => {
    expect(resolveInitClientId("player-1", "player-2", true)).toBe("player-2");
  });
});
