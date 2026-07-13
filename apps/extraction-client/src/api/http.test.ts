import { describe, expect, it } from "vitest";

import {
  ApiError,
  CLIENT_API_ERROR_CODES,
  createHttpClient,
  decodeApiErrorBody,
} from "./http";

describe("HTTP API client", () => {
  it("sends JSON with credentials, headers, and a timeout signal", async () => {
    const calls: Array<{ options?: RequestInit; url: string }> = [];
    const request = (async (
      input: RequestInfo | URL,
      options?: RequestInit,
    ) => {
      calls.push({ options, url: String(input) });
      return jsonResponse({ accepted: true });
    }) as typeof fetch;
    const client = createHttpClient({
      baseUrl: "https://api.example.test/",
      request,
      timeoutMs: 1_000,
    });

    const result = await client.requestJson(
      "/resource",
      { body: { value: 7 }, method: "POST" },
      (value) => value,
    );

    expect(result).toEqual({ accepted: true });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.url).toBe("https://api.example.test/resource");
    expect(calls[0]?.options).toMatchObject({
      body: JSON.stringify({ value: 7 }),
      credentials: "include",
      method: "POST",
    });
    expect(new Headers(calls[0]?.options?.headers).get("Accept")).toBe(
      "application/json",
    );
    expect(new Headers(calls[0]?.options?.headers).get("Content-Type")).toBe(
      "application/json",
    );
    expect(calls[0]?.options?.signal).toBeInstanceOf(AbortSignal);
  });

  it("decodes the stable server error envelope", async () => {
    const client = createHttpClient({
      request: responseRequest(
        jsonResponse(
          {
            error: { code: "AUTH_WRONG_NETWORK", retryable: false },
          },
          422,
        ),
      ),
    });

    await expect(
      client.requestJson("/protected", { method: "GET" }, (value) => value),
    ).rejects.toMatchObject({
      code: "AUTH_WRONG_NETWORK",
      kind: "server",
      name: "ApiError",
      retryable: false,
      status: 422,
    });
  });

  it("rejects malformed success and error bodies with a stable client code", async () => {
    const successClient = createHttpClient({
      request: responseRequest(
        new Response("not-json", {
          headers: { "Content-Type": "text/plain" },
          status: 200,
        }),
      ),
    });
    await expect(
      successClient.requestJson("/data", { method: "GET" }, (value) => value),
    ).rejects.toMatchObject({
      code: CLIENT_API_ERROR_CODES.malformedResponse,
      kind: "malformed-response",
      retryable: false,
      status: 200,
    });

    const errorClient = createHttpClient({
      request: responseRequest(jsonResponse({ message: "bad" }, 503)),
    });
    await expect(
      errorClient.requestJson("/data", { method: "GET" }, (value) => value),
    ).rejects.toMatchObject({
      code: CLIENT_API_ERROR_CODES.malformedResponse,
      kind: "malformed-response",
      retryable: true,
      status: 503,
    });
  });

  it("classifies timeout and transport failures", async () => {
    const timeoutRequest = ((
      _input: RequestInfo | URL,
      options?: RequestInit,
    ) =>
      new Promise<Response>((_resolve, reject) => {
        options?.signal?.addEventListener(
          "abort",
          () => reject(options.signal?.reason),
          { once: true },
        );
      })) as typeof fetch;
    const timeoutClient = createHttpClient({
      request: timeoutRequest,
      timeoutMs: 5,
    });
    await expect(
      timeoutClient.requestJson("/slow", { method: "GET" }, (value) => value),
    ).rejects.toMatchObject({
      code: CLIENT_API_ERROR_CODES.timeout,
      kind: "timeout",
      retryable: true,
      status: null,
    });

    const networkClient = createHttpClient({
      request: (async () => {
        throw new TypeError("offline");
      }) as typeof fetch,
    });
    await expect(
      networkClient.requestJson(
        "/offline",
        { method: "GET" },
        (value) => value,
      ),
    ).rejects.toMatchObject({
      code: CLIENT_API_ERROR_CODES.network,
      kind: "network",
      retryable: true,
      status: null,
    });
  });

  it("strictly decodes API errors and requires an exact 204", async () => {
    expect(() =>
      decodeApiErrorBody({
        error: {
          code: "AUTH_REQUIRED",
          retryable: false,
          unexpected: true,
        },
      }),
    ).toThrow("response.error: unknown field unexpected");

    const client = createHttpClient({
      request: responseRequest(jsonResponse(true)),
    });
    await expect(
      client.requestNoContent("/logout", { method: "POST" }),
    ).rejects.toBeInstanceOf(ApiError);
  });
});

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    headers: { "Content-Type": "application/json; charset=utf-8" },
    status,
  });
}

function responseRequest(response: Response): typeof fetch {
  return (async () => response) as typeof fetch;
}
