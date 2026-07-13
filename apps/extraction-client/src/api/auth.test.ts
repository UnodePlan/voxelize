import { describe, expect, it } from "vitest";

import { ETHEREUM_MAINNET_CHAIN_ID, createAuthApi } from "./auth";
import { CLIENT_API_ERROR_CODES } from "./http";

const ADDRESS = "0x0000000000000000000000000000000000000001";
const EXPIRES_AT = "2027-01-15T08:00:00Z";
const NONCE = "a1b2c3d4e5f6g7h8";

describe("auth API", () => {
  it("maps the four auth endpoints and never forwards extra verify fields", async () => {
    const calls: Array<{ options?: RequestInit; url: string }> = [];
    const request = (async (
      input: RequestInfo | URL,
      options?: RequestInit,
    ) => {
      const url = String(input);
      calls.push({ options, url });
      if (url.endsWith("/siwe/nonce")) {
        return jsonResponse({ expiresAt: EXPIRES_AT, nonce: NONCE });
      }
      if (url.endsWith("/siwe/verify")) {
        return jsonResponse(true);
      }
      if (url.endsWith("/session")) {
        return jsonResponse({
          address: ADDRESS,
          chainId: ETHEREUM_MAINNET_CHAIN_ID,
        });
      }
      return new Response(null, { status: 204 });
    }) as typeof fetch;
    const api = createAuthApi({
      baseUrl: "https://api.example.test",
      request,
      timeoutMs: 1_000,
    });

    await expect(api.getNonce()).resolves.toEqual({
      expiresAt: EXPIRES_AT,
      nonce: NONCE,
    });
    const verifyInput = {
      cacao: { forged: true },
      message: "example.test wants you to sign in",
      signature: "0x01",
    };
    await expect(api.verifyMessage(verifyInput)).resolves.toBe(true);
    await expect(api.getSession()).resolves.toEqual({
      address: ADDRESS,
      chainId: 1,
    });
    await expect(api.logout()).resolves.toBeUndefined();

    expect(calls.map(({ url }) => url)).toEqual([
      "https://api.example.test/api/auth/siwe/nonce",
      "https://api.example.test/api/auth/siwe/verify",
      "https://api.example.test/api/auth/session",
      "https://api.example.test/api/auth/logout",
    ]);
    expect(calls.map(({ options }) => options?.method)).toEqual([
      "GET",
      "POST",
      "GET",
      "POST",
    ]);
    expect(
      calls.every(({ options }) => options?.credentials === "include"),
    ).toBe(true);
    expect(
      calls.every(({ options }) => options?.signal instanceof AbortSignal),
    ).toBe(true);
    expect(JSON.parse(String(calls[1]?.options?.body))).toEqual({
      message: "example.test wants you to sign in",
      signature: "0x01",
    });
  });

  it("accepts a missing session and rejects malformed auth responses", async () => {
    await expect(apiReturning(null).getSession()).resolves.toBeNull();

    const malformedCases: Array<{
      call: "getNonce" | "getSession" | "verifyMessage";
      value: unknown;
    }> = [
      {
        call: "getNonce",
        value: { expiresAt: EXPIRES_AT, nonce: "short" },
      },
      {
        call: "getNonce",
        value: { expiresAt: "tomorrow", nonce: NONCE },
      },
      {
        call: "getNonce",
        value: { expiresAt: EXPIRES_AT, nonce: NONCE, extra: true },
      },
      {
        call: "getSession",
        value: { address: "0x01", chainId: 1 },
      },
      {
        call: "getSession",
        value: { address: ADDRESS, chainId: 5 },
      },
      {
        call: "verifyMessage",
        value: false,
      },
    ];

    for (const testCase of malformedCases) {
      const api = apiReturning(testCase.value);
      const result =
        testCase.call === "verifyMessage"
          ? api.verifyMessage({ message: "message", signature: "0x01" })
          : api[testCase.call]();
      await expect(result).rejects.toMatchObject({
        code: CLIENT_API_ERROR_CODES.malformedResponse,
        kind: "malformed-response",
        status: 200,
      });
    }
  });

  it("requires non-blank verification input before making a request", async () => {
    let requests = 0;
    const api = createAuthApi({
      request: (async () => {
        requests += 1;
        return jsonResponse(true);
      }) as typeof fetch,
    });

    await expect(
      api.verifyMessage({ message: " ", signature: "0x01" }),
    ).rejects.toThrow("message: expected non-blank string");
    expect(requests).toBe(0);
  });

  it("requires logout to return 204", async () => {
    const api = createAuthApi({
      request: (async () => jsonResponse(true)) as typeof fetch,
    });

    await expect(api.logout()).rejects.toMatchObject({
      code: CLIENT_API_ERROR_CODES.malformedResponse,
      kind: "malformed-response",
      status: 200,
    });
  });
});

function apiReturning(value: unknown) {
  return createAuthApi({
    request: (async () => jsonResponse(value)) as typeof fetch,
  });
}

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    headers: { "Content-Type": "application/json" },
    status: 200,
  });
}
