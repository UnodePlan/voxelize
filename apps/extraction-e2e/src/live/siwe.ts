import { keccak256, toUtf8Bytes, Wallet } from "ethers";
import { SiweMessage } from "siwe";

import {
  assertOnlyKeys,
  readNonEmptyString,
  readRecord,
} from "../../../../contracts/extraction/v1/decoder-utils";

import type { LiveE2eConfig } from "./config";
import { requestJson, type LiveHttpTransport } from "./http";

const NONCE_PATTERN = /^[a-z0-9]{8,}$/iu;
const ADDRESS_PATTERN = /^0x[0-9a-f]{40}$/iu;
const NONCE_TTL_MS = 5 * 60_000;
const MESSAGE_LIFETIME_MS = 4 * 60_000;

interface IssuedNonce {
  expiresAt: string;
  nonce: string;
}

export function deterministicLiveWallet(index: number): Wallet {
  if (!Number.isSafeInteger(index) || index < 0) {
    throw new Error("live wallet index must be a non-negative safe integer");
  }
  // 该私钥由公开标签确定，只用于无资产的本地 E2E 身份，绝不能接收真实资产。
  const privateKey = keccak256(
    toUtf8Bytes(`voxel-extraction-live-e2e-wallet:${index}`),
  );
  return new Wallet(privateKey);
}

export async function authenticateEoa(
  transport: LiveHttpTransport,
  wallet: Wallet,
  config: LiveE2eConfig,
): Promise<void> {
  const nonce = await requestJson(
    transport,
    "/api/auth/siwe/nonce",
    { method: "GET" },
    decodeNonce,
  );
  const message = createSiweMessage(wallet.address, nonce, config);
  const signature = await wallet.signMessage(message);
  await requestJson(
    transport,
    "/api/auth/siwe/verify",
    { method: "POST", body: { message, signature } },
    decodeVerified,
  );
  const session = await requestJson(
    transport,
    "/api/auth/session",
    { method: "GET" },
    decodeSession,
  );
  if (session.address.toLowerCase() !== wallet.address.toLowerCase()) {
    throw new Error("SIWE session address does not match the signing wallet");
  }
}

function createSiweMessage(
  address: string,
  nonce: IssuedNonce,
  config: LiveE2eConfig,
): string {
  const { expirationTime, issuedAt } = deriveE2eSiweWindow(nonce.expiresAt);
  return new SiweMessage({
    address,
    chainId: 1,
    domain: new URL(config.publicOrigin).host,
    expirationTime,
    issuedAt,
    nonce: nonce.nonce,
    statement: "Sign in to Voxel Extraction.",
    uri: config.publicOrigin,
    version: "1",
  }).prepareMessage();
}

export function deriveE2eSiweWindow(expiresAt: string): {
  expirationTime: string;
  issuedAt: string;
} {
  const nonceExpiryMs = Date.parse(expiresAt);
  if (!Number.isFinite(nonceExpiryMs)) {
    throw new Error("SIWE nonce expiry must be an ISO timestamp");
  }
  // 受控时钟会快于本机墙钟；nonce 到期时间是该测试环境可靠的服务端时间锚点。
  const issuedAtMs = nonceExpiryMs - NONCE_TTL_MS;
  const expirationMs = Math.min(
    issuedAtMs + MESSAGE_LIFETIME_MS,
    nonceExpiryMs - 1_000,
  );
  if (expirationMs <= issuedAtMs) {
    throw new Error("SIWE nonce expires before a message can be signed");
  }
  return {
    expirationTime: new Date(expirationMs).toISOString(),
    issuedAt: new Date(issuedAtMs).toISOString(),
  };
}

function decodeNonce(value: unknown): IssuedNonce {
  const source = readRecord(value, "liveNonceResponse");
  assertOnlyKeys(source, ["nonce", "expiresAt"], "liveNonceResponse");
  const nonce = readNonEmptyString(source.nonce, "liveNonceResponse.nonce");
  const expiresAt = readNonEmptyString(
    source.expiresAt,
    "liveNonceResponse.expiresAt",
  );
  if (!NONCE_PATTERN.test(nonce)) {
    throw new Error("liveNonceResponse.nonce: expected SIWE nonce");
  }
  if (!Number.isFinite(Date.parse(expiresAt))) {
    throw new Error("liveNonceResponse.expiresAt: expected timestamp");
  }
  return { expiresAt, nonce };
}

function decodeVerified(value: unknown): true {
  if (value !== true) throw new Error("liveVerifyResponse: expected true");
  return true;
}

function decodeSession(value: unknown): { address: string; chainId: 1 } {
  const source = readRecord(value, "liveSessionResponse");
  assertOnlyKeys(source, ["address", "chainId"], "liveSessionResponse");
  const address = readNonEmptyString(
    source.address,
    "liveSessionResponse.address",
  );
  if (!ADDRESS_PATTERN.test(address) || source.chainId !== 1) {
    throw new Error("liveSessionResponse: expected a Mainnet EOA session");
  }
  return { address, chainId: 1 };
}
