import { describe, expect, it } from "vitest";

import manifestJson from "../../../contracts/extraction/v1/manifest.json";

import { fetchExtractionManifest } from "./api";

describe("bootstrap API", () => {
  it("checks readiness before decoding the bootstrap manifest", async () => {
    const requests: Array<{ options?: RequestInit; url: string }> = [];
    const request = (async (
      input: RequestInfo | URL,
      options?: RequestInit,
    ) => {
      const url = String(input);
      requests.push({ options, url });
      if (url.endsWith("/health/ready")) {
        return new Response(null, { status: 204 });
      }
      return jsonResponse(manifestJson);
    }) as typeof fetch;

    const manifest = await fetchExtractionManifest(request, 1_000);

    expect(requests.map(({ url }) => url)).toEqual([
      "/health/ready",
      "/api/bootstrap",
    ]);
    expect(requests.every(({ options }) => options?.signal !== undefined)).toBe(
      true,
    );
    expect(manifest.resources.map(({ key }) => key)).toEqual([
      "dirt",
      "gold",
      "diamond",
    ]);
  });

  it("does not request bootstrap when readiness fails", async () => {
    const urls: string[] = [];
    const request = (async (input: RequestInfo | URL) => {
      urls.push(String(input));
      return new Response(null, { status: 503 });
    }) as typeof fetch;

    await expect(fetchExtractionManifest(request, 1_000)).rejects.toThrow(
      "readiness request failed with status 503",
    );
    expect(urls).toEqual(["/health/ready"]);
  });

  it("aborts a readiness request that never completes", async () => {
    const request = ((_input: RequestInfo | URL, options?: RequestInit) =>
      new Promise<Response>((_resolve, reject) => {
        options?.signal?.addEventListener(
          "abort",
          () => reject(new Error("request aborted")),
          { once: true },
        );
      })) as typeof fetch;

    await expect(fetchExtractionManifest(request, 5)).rejects.toThrow(
      "request aborted",
    );
  });
});

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    headers: { "Content-Type": "application/json" },
    status: 200,
  });
}
