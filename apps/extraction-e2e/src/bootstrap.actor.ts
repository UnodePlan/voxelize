import { describe, expect, it } from "vitest";

import manifestJson from "../../../contracts/extraction/v1/manifest.json";

import { readBootstrapContract, type FetchBootstrap } from "./bootstrap-client";

describe("extraction actor bootstrap", () => {
  it("decodes the server bootstrap through the shared contract boundary", async () => {
    const request: FetchBootstrap = async (url) => {
      expect(url).toBe("http://127.0.0.1:4100/api/bootstrap");
      return {
        ok: true,
        status: 200,
        json: async () => manifestJson as unknown,
      };
    };

    const manifest = await readBootstrapContract(
      "http://127.0.0.1:4100/",
      request,
    );
    expect(manifest.resources.map(({ key }) => key)).toEqual([
      "dirt",
      "gold",
      "diamond",
    ]);
  });

  it("fails closed when bootstrap is unavailable", async () => {
    const request: FetchBootstrap = async () => ({
      ok: false,
      status: 503,
      json: async () => ({}),
    });

    await expect(
      readBootstrapContract("http://127.0.0.1:4100", request),
    ).rejects.toThrow("status 503");
  });
});
