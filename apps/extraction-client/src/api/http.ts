import {
  assertOnlyKeys,
  readBoolean,
  readEnum,
  readRecord,
} from "../../../../contracts/extraction/v1/decoder-utils";
import {
  ERROR_CODES,
  type ErrorCode,
} from "../../../../contracts/extraction/v1/types";

export const DEFAULT_API_TIMEOUT_MS = 5_000;

export const CLIENT_API_ERROR_CODES = {
  malformedResponse: "CLIENT_RESPONSE_MALFORMED",
  network: "CLIENT_NETWORK_ERROR",
  timeout: "CLIENT_REQUEST_TIMEOUT",
} as const;

export type ClientApiErrorCode =
  (typeof CLIENT_API_ERROR_CODES)[keyof typeof CLIENT_API_ERROR_CODES];
export type ApiErrorCode = ErrorCode | ClientApiErrorCode;
export type ApiErrorKind =
  | "server"
  | "malformed-response"
  | "network"
  | "timeout";

export class ApiError extends Error {
  readonly code: ApiErrorCode;
  readonly kind: ApiErrorKind;
  readonly retryable: boolean;
  readonly status: number | null;

  constructor(
    kind: ApiErrorKind,
    code: ApiErrorCode,
    status: number | null,
    retryable: boolean,
    cause?: unknown,
  ) {
    super(code, { cause });
    this.name = "ApiError";
    this.kind = kind;
    this.code = code;
    this.status = status;
    this.retryable = retryable;
  }
}

export interface DecodedApiError {
  code: ErrorCode;
  retryable: boolean;
}

export type ApiMethod = "DELETE" | "GET" | "POST";
export type JsonDecoder<T> = (value: unknown) => T;

export interface ApiRequestOptions {
  body?: unknown;
  method: ApiMethod;
}

export interface HttpClientOptions {
  baseUrl?: string;
  request?: typeof fetch;
  timeoutMs?: number;
}

export interface HttpClient {
  requestJson<T>(
    path: string,
    options: ApiRequestOptions,
    decode: JsonDecoder<T>,
  ): Promise<T>;
  requestNoContent(path: string, options: ApiRequestOptions): Promise<void>;
}

export function createHttpClient({
  baseUrl = "",
  request = globalThis.fetch,
  timeoutMs = DEFAULT_API_TIMEOUT_MS,
}: HttpClientOptions = {}): HttpClient {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error("timeoutMs: expected positive safe integer");
  }
  const normalizedBaseUrl = baseUrl.replace(/\/$/u, "");

  return {
    requestJson: async <T>(
      path: string,
      options: ApiRequestOptions,
      decode: JsonDecoder<T>,
    ) => {
      const signal = AbortSignal.timeout(timeoutMs);
      const response = await performRequest(
        request,
        `${normalizedBaseUrl}${validatePath(path)}`,
        options,
        signal,
      );
      if (!response.ok) {
        return throwResponseError(response, signal);
      }
      return decodeJsonResponse(response, signal, decode);
    },
    requestNoContent: async (path: string, options: ApiRequestOptions) => {
      const signal = AbortSignal.timeout(timeoutMs);
      const response = await performRequest(
        request,
        `${normalizedBaseUrl}${validatePath(path)}`,
        options,
        signal,
      );
      if (!response.ok) {
        return throwResponseError(response, signal);
      }
      if (response.status !== 204) {
        throw malformedResponse(response.status);
      }
    },
  };
}

export function decodeApiErrorBody(value: unknown): DecodedApiError {
  const root = readRecord(value, "response");
  assertOnlyKeys(root, ["error"], "response");
  const error = readRecord(root.error, "response.error");
  assertOnlyKeys(error, ["code", "retryable"], "response.error");
  return {
    code: readEnum(error.code, ERROR_CODES, "response.error.code"),
    retryable: readBoolean(error.retryable, "response.error.retryable"),
  };
}

async function performRequest(
  request: typeof fetch,
  url: string,
  options: ApiRequestOptions,
  signal: AbortSignal,
): Promise<Response> {
  const hasBody = options.body !== undefined;
  const headers: Record<string, string> = { Accept: "application/json" };
  if (hasBody) {
    headers["Content-Type"] = "application/json";
  }

  try {
    return await request(url, {
      body: hasBody ? JSON.stringify(options.body) : undefined,
      credentials: "include",
      headers,
      method: options.method,
      signal,
    });
  } catch (error) {
    if (signal.aborted) {
      throw new ApiError(
        "timeout",
        CLIENT_API_ERROR_CODES.timeout,
        null,
        true,
        error,
      );
    }
    throw new ApiError(
      "network",
      CLIENT_API_ERROR_CODES.network,
      null,
      true,
      error,
    );
  }
}

async function decodeJsonResponse<T>(
  response: Response,
  signal: AbortSignal,
  decode: JsonDecoder<T>,
): Promise<T> {
  try {
    return decode(await readJson(response));
  } catch (error) {
    if (signal.aborted) {
      throw new ApiError(
        "timeout",
        CLIENT_API_ERROR_CODES.timeout,
        null,
        true,
        error,
      );
    }
    throw malformedResponse(response.status, error);
  }
}

async function throwResponseError(
  response: Response,
  signal: AbortSignal,
): Promise<never> {
  try {
    const decoded = decodeApiErrorBody(await readJson(response));
    throw new ApiError(
      "server",
      decoded.code,
      response.status,
      decoded.retryable,
    );
  } catch (error) {
    if (error instanceof ApiError) {
      throw error;
    }
    if (signal.aborted) {
      throw new ApiError(
        "timeout",
        CLIENT_API_ERROR_CODES.timeout,
        null,
        true,
        error,
      );
    }
    throw malformedResponse(response.status, error);
  }
}

async function readJson(response: Response): Promise<unknown> {
  const contentType = response.headers.get("Content-Type")?.split(";", 1)[0];
  if (contentType?.trim().toLowerCase() !== "application/json") {
    throw new Error("response: expected application/json");
  }
  return (await response.json()) as unknown;
}

function malformedResponse(status: number, cause?: unknown): ApiError {
  return new ApiError(
    "malformed-response",
    CLIENT_API_ERROR_CODES.malformedResponse,
    status,
    status >= 500,
    cause,
  );
}

function validatePath(path: string): string {
  if (!path.startsWith("/") || path.startsWith("//")) {
    throw new Error("path: expected root-relative path");
  }
  return path;
}
