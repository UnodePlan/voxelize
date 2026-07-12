import {
  decodeExtractionManifest,
  type ExtractionManifest,
} from "../../../contracts/extraction/v1/typescript";

const REQUEST_TIMEOUT_MS = 5_000;

export async function fetchExtractionManifest(
  request: typeof fetch = globalThis.fetch,
  timeoutMs = REQUEST_TIMEOUT_MS,
): Promise<ExtractionManifest> {
  const baseUrl = apiBaseUrl();
  const signal = AbortSignal.timeout(timeoutMs);
  const requestOptions: RequestInit = {
    credentials: "include",
    headers: { Accept: "application/json" },
    signal,
  };

  const readiness = await request(`${baseUrl}/health/ready`, requestOptions);
  if (!readiness.ok) {
    throw new Error(`readiness request failed with status ${readiness.status}`);
  }

  const response = await request(`${baseUrl}/api/bootstrap`, requestOptions);

  if (!response.ok) {
    throw new Error(`bootstrap request failed with status ${response.status}`);
  }

  return decodeExtractionManifest((await response.json()) as unknown);
}

export function apiBaseUrl(
  value = import.meta.env.VITE_EXTRACTION_API_URL,
): string {
  if (value === undefined || value.trim() === "") {
    return "";
  }

  const url = new URL(value);
  return url.toString().replace(/\/$/, "");
}
