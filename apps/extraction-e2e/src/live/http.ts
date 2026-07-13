import type { APIRequestContext } from "@playwright/test";

import { decodeQueueSnapshot } from "../../../extraction-client/src/api/game-decoders";
import { decodeApiErrorBody } from "../../../extraction-client/src/api/http";
import type { QueueSnapshot } from "../../../extraction-client/src/api/models";

export type LiveHttpMethod = "GET" | "POST";

export interface LiveHttpRequest {
  body?: unknown;
  method: LiveHttpMethod;
  timeoutMs?: number;
}

export interface LiveHttpResponse {
  body: unknown;
  setCookie: string | null;
  status: number;
}

export interface LiveHttpTransport {
  request(path: string, request: LiveHttpRequest): Promise<LiveHttpResponse>;
}

export class LiveApiError extends Error {
  constructor(
    readonly code: string,
    readonly retryable: boolean,
    readonly status: number,
  ) {
    super(`live API request failed: ${code}`);
    this.name = "LiveApiError";
  }
}

export class CookieHttpTransport implements LiveHttpTransport {
  private cookie: string | null = null;

  constructor(
    private readonly baseUrl: string,
    private readonly publicOrigin: string,
    private readonly timeoutMs: number,
  ) {}

  get sessionCookie(): string {
    if (this.cookie === null) {
      throw new Error("live HTTP session cookie is unavailable");
    }
    return this.cookie;
  }

  async request(
    path: string,
    request: LiveHttpRequest,
  ): Promise<LiveHttpResponse> {
    const timeoutMs = resolveRequestTimeout(request.timeoutMs, this.timeoutMs);
    const headers: Record<string, string> = {
      Accept: "application/json",
      Origin: this.publicOrigin,
    };
    if (request.body !== undefined)
      headers["Content-Type"] = "application/json";
    if (this.cookie !== null) headers.Cookie = this.cookie;
    const response = await fetch(resolvePath(this.baseUrl, path), {
      body:
        request.body === undefined ? undefined : JSON.stringify(request.body),
      headers,
      method: request.method,
      redirect: "error",
      signal: AbortSignal.timeout(timeoutMs),
    });
    const setCookie = response.headers.get("set-cookie");
    if (setCookie !== null) this.captureCookie(setCookie);
    return {
      body: await readFetchBody(response),
      setCookie,
      status: response.status,
    };
  }

  private captureCookie(value: string): void {
    const cookie = value.split(";", 1)[0]?.trim();
    if (cookie === undefined || !/^[^=\s]+=[^\s]+$/u.test(cookie)) {
      throw new Error("live authentication returned an invalid session cookie");
    }
    this.cookie = cookie;
  }
}

export class PlaywrightHttpTransport implements LiveHttpTransport {
  constructor(
    private readonly requestContext: APIRequestContext,
    private readonly baseUrl: string,
    private readonly publicOrigin: string,
    private readonly timeoutMs: number,
  ) {}

  async request(
    path: string,
    request: LiveHttpRequest,
  ): Promise<LiveHttpResponse> {
    const timeoutMs = resolveRequestTimeout(request.timeoutMs, this.timeoutMs);
    const response = await this.requestContext.fetch(
      resolvePath(this.baseUrl, path),
      {
        data: request.body,
        failOnStatusCode: false,
        headers: {
          Accept: "application/json",
          Origin: this.publicOrigin,
        },
        method: request.method,
        timeout: timeoutMs,
      },
    );
    const headers = response.headers();
    return {
      body: await readPlaywrightBody(response),
      setCookie: headers["set-cookie"] ?? null,
      status: response.status(),
    };
  }
}

export async function requestJson<T>(
  transport: LiveHttpTransport,
  path: string,
  request: LiveHttpRequest,
  decode: (value: unknown) => T,
): Promise<T> {
  const response = await transport.request(path, request);
  if (response.status < 200 || response.status >= 300) {
    let error;
    try {
      error = decodeApiErrorBody(response.body);
    } catch (cause) {
      throw new Error(
        `live API returned malformed error body (${response.status})`,
        { cause },
      );
    }
    throw new LiveApiError(error.code, error.retryable, response.status);
  }
  try {
    return decode(response.body);
  } catch (cause) {
    throw new Error(`live API returned malformed success body for ${path}`, {
      cause,
    });
  }
}

export function fetchQueue(
  transport: LiveHttpTransport,
  method: LiveHttpMethod,
): Promise<QueueSnapshot> {
  return requestJson(
    transport,
    "/api/matchmaking/queue",
    { method },
    decodeQueueSnapshot,
  );
}

async function readFetchBody(response: Response): Promise<unknown> {
  if (response.status === 204) return null;
  requireJsonContentType(response.headers.get("content-type"));
  return (await response.json()) as unknown;
}

async function readPlaywrightBody(
  response: Awaited<ReturnType<APIRequestContext["fetch"]>>,
): Promise<unknown> {
  if (response.status() === 204) return null;
  requireJsonContentType(response.headers()["content-type"] ?? null);
  return (await response.json()) as unknown;
}

function requireJsonContentType(value: string | null): void {
  const mediaType = value?.split(";", 1)[0]?.trim().toLowerCase();
  if (mediaType !== "application/json") {
    throw new Error("live API response must use application/json");
  }
}

function resolvePath(baseUrl: string, path: string): string {
  if (!path.startsWith("/") || path.startsWith("//")) {
    throw new Error("live API path must be root-relative");
  }
  return new URL(path, baseUrl).toString();
}

function resolveRequestTimeout(
  override: number | undefined,
  fallback: number,
): number {
  const timeoutMs = override ?? fallback;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error("live HTTP timeout must be a positive safe integer");
  }
  return timeoutMs;
}
