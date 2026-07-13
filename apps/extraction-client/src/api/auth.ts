import {
  assertOnlyKeys,
  readNonEmptyString,
  readRecord,
} from "../../../../contracts/extraction/v1/decoder-utils";
import { apiBaseUrl } from "../api";

import { createHttpClient, type HttpClientOptions } from "./http";

export const ETHEREUM_MAINNET_CHAIN_ID = 1;

const ETHEREUM_ADDRESS_PATTERN = /^0x[0-9a-f]{40}$/iu;
const RFC3339_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/u;
const SIWE_NONCE_PATTERN = /^[a-z0-9]{8,}$/iu;

export interface SiweNonce {
  expiresAt: string;
  nonce: string;
}

export interface AuthSession {
  address: string;
  chainId: typeof ETHEREUM_MAINNET_CHAIN_ID;
}

export interface VerifySiweMessage {
  message: string;
  signature: string;
}

export interface AuthApi {
  getNonce(): Promise<SiweNonce>;
  getSession(): Promise<AuthSession | null>;
  logout(): Promise<void>;
  verifyMessage(input: VerifySiweMessage): Promise<true>;
}

export type AuthApiOptions = HttpClientOptions;

export function createAuthApi({
  baseUrl = apiBaseUrl(),
  ...options
}: AuthApiOptions = {}): AuthApi {
  const http = createHttpClient({ baseUrl, ...options });

  return {
    getNonce: () =>
      http.requestJson("/api/auth/siwe/nonce", { method: "GET" }, decodeNonce),
    getSession: () =>
      http.requestJson("/api/auth/session", { method: "GET" }, decodeSession),
    logout: () => http.requestNoContent("/api/auth/logout", { method: "POST" }),
    verifyMessage: async (input) =>
      http.requestJson(
        "/api/auth/siwe/verify",
        {
          body: {
            message: requireNonBlank(input.message, "message"),
            signature: requireNonBlank(input.signature, "signature"),
          },
          method: "POST",
        },
        decodeVerified,
      ),
  };
}

function decodeNonce(value: unknown): SiweNonce {
  const source = readRecord(value, "nonceResponse");
  assertOnlyKeys(source, ["nonce", "expiresAt"], "nonceResponse");
  const nonce = readNonEmptyString(source.nonce, "nonceResponse.nonce");
  if (!SIWE_NONCE_PATTERN.test(nonce)) {
    throw new Error("nonceResponse.nonce: expected SIWE nonce");
  }
  const expiresAt = readNonEmptyString(
    source.expiresAt,
    "nonceResponse.expiresAt",
  );
  if (
    !RFC3339_PATTERN.test(expiresAt) ||
    !Number.isFinite(Date.parse(expiresAt))
  ) {
    throw new Error("nonceResponse.expiresAt: expected RFC3339 timestamp");
  }
  return { expiresAt, nonce };
}

function decodeSession(value: unknown): AuthSession | null {
  if (value === null) {
    return null;
  }
  const source = readRecord(value, "sessionResponse");
  assertOnlyKeys(source, ["address", "chainId"], "sessionResponse");
  const address = readNonEmptyString(source.address, "sessionResponse.address");
  if (!ETHEREUM_ADDRESS_PATTERN.test(address)) {
    throw new Error("sessionResponse.address: expected Ethereum address");
  }
  if (source.chainId !== ETHEREUM_MAINNET_CHAIN_ID) {
    throw new Error("sessionResponse.chainId: expected Ethereum Mainnet");
  }
  return { address, chainId: ETHEREUM_MAINNET_CHAIN_ID };
}

function decodeVerified(value: unknown): true {
  if (value !== true) {
    throw new Error("verifyResponse: expected true");
  }
  return true;
}

function requireNonBlank(value: string, path: string): string {
  if (value.trim() === "") {
    throw new Error(`${path}: expected non-blank string`);
  }
  return value;
}
