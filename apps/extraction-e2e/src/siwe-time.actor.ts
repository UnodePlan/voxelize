import { describe, expect, it } from "vitest";

import { deriveE2eSiweWindow } from "./live/siwe";

describe("controlled-clock SIWE timestamps", () => {
  it("derives the signing window from the server-issued nonce expiry", () => {
    expect(deriveE2eSiweWindow("2030-01-01T00:05:00.000Z")).toEqual({
      issuedAt: "2030-01-01T00:00:00.000Z",
      expirationTime: "2030-01-01T00:04:00.000Z",
    });
  });

  it("rejects an invalid nonce expiry", () => {
    expect(() => deriveE2eSiweWindow("not-a-time")).toThrow(/ISO timestamp/u);
  });
});
