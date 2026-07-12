import {
  decodeExtractionManifest,
  type ExtractionManifest,
} from "../../../contracts/extraction/v1/typescript";

export interface FetchResponse {
  ok: boolean;
  status: number;
  json(): Promise<unknown>;
}

export type FetchBootstrap = (url: string) => Promise<FetchResponse>;

export async function readBootstrapContract(
  baseUrl: string,
  request: FetchBootstrap,
): Promise<ExtractionManifest> {
  const response = await request(`${baseUrl.replace(/\/$/, "")}/api/bootstrap`);
  if (!response.ok) {
    throw new Error(`bootstrap request failed with status ${response.status}`);
  }
  return decodeExtractionManifest(await response.json());
}
