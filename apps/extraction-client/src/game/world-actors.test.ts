import { describe, expect, it } from "vitest";

import { peerDisplayName, peerPalette } from "./world-actors";

describe("peerDisplayName", () => {
  it("prefers username and falls back to short id", () => {
    expect(peerDisplayName("abcdef012345", "Alice")).toBe("Alice");
    expect(peerDisplayName("abcdef012345")).toBe("P-abcdef");
    expect(peerDisplayName("ab")).toBe("ab");
  });
});

describe("peerPalette", () => {
  it("is stable for the same id", () => {
    expect(peerPalette("peer-1")).toEqual(peerPalette("peer-1"));
    expect(peerPalette("peer-1").body).not.toBe(peerPalette("peer-2").body);
  });
});
