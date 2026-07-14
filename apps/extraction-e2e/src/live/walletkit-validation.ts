import { buildApprovedNamespaces, getSdkError } from "@walletconnect/utils";
import { getAddress, getBytes, isHexString, sha256, toUtf8Bytes } from "ethers";
import { SiweMessage } from "siwe";

export const MAINNET_CAIP_CHAIN = "eip155:1" as const;
export const SIWE_METHOD = "personal_sign" as const;
export const DEFAULT_SIWE_STATEMENT = "Sign in to Voxel Extraction.";

const MAX_SIWE_LIFETIME_MS = 5 * 60_000;
const MAX_CLOCK_SKEW_MS = 60_000;
const NONCE_PATTERN = /^[a-z0-9]{8,}$/iu;
const SAFE_EVENTS = ["accountsChanged", "chainChanged"];
const UNKNOWN_METHOD = "unknown_method";
const AUDITABLE_METHODS = new Set([
  SIWE_METHOD,
  "eth_sign",
  "eth_signTypedData",
  "eth_signTypedData_v3",
  "eth_signTypedData_v4",
  "eth_sendTransaction",
  "eth_signTransaction",
  "eth_sendRawTransaction",
  "wallet_sendCalls",
  "wallet_grantPermissions",
  "wallet_revokePermissions",
  "wallet_requestPermissions",
  "wallet_getPermissions",
  "wallet_addEthereumChain",
  "wallet_switchEthereumChain",
  "wallet_watchAsset",
]);
const UNSAFE_METHODS = new Set([
  "eth_sendTransaction",
  "eth_signTransaction",
  "eth_sendRawTransaction",
  "wallet_sendCalls",
  "wallet_grantPermissions",
  "wallet_revokePermissions",
  "wallet_requestPermissions",
  "wallet_getPermissions",
  "wallet_addEthereumChain",
  "wallet_switchEthereumChain",
  "wallet_watchAsset",
]);

type NamespaceProposal = Parameters<
  typeof buildApprovedNamespaces
>[0]["proposal"];

interface MessageSigner {
  address: string;
  signMessage(message: string | Uint8Array): Promise<string>;
}

export interface SiweExpectation {
  address: string;
  expectedOrigin: string;
  expectedStatement?: string;
  nowMs?: number;
}

export interface SanitizedSiweEvidence {
  address: string;
  chainId: 1;
  domain: string;
  expirationTime: string;
  issuedAt: string;
  nonceSha256: string;
  statement: string;
  uri: string;
  version: "1";
}

export interface WalletRequestAuditSnapshot {
  rejectedMethods: readonly string[];
  rejectedRequestCount: number;
  rejectedUnsafeRequestCount: number;
  requestedMethods: readonly string[];
  signedMessageCount: number;
}

export interface WalletKitEvidence extends WalletRequestAuditSnapshot {
  address: string;
  approvedChains: readonly [typeof MAINNET_CAIP_CHAIN];
  approvedMethods: readonly [typeof SIWE_METHOD];
  chainId: 1;
  pairingUriSha256: string;
  siwe: SanitizedSiweEvidence;
}

export type WalletJsonRpcResponse =
  | { id: number; jsonrpc: "2.0"; result: string }
  | {
      error: { code: number; message: string };
      id: number;
      jsonrpc: "2.0";
    };

export interface WalletSessionRequest {
  chainId: unknown;
  id: number;
  request: { method: unknown; params: unknown };
}

export interface WalletSessionRequestResult {
  response: WalletJsonRpcResponse;
  siwe?: SanitizedSiweEvidence;
}

export interface WalletConnectVerifyContext {
  verified: {
    isScam?: boolean;
    origin: string;
    validation: "INVALID" | "UNKNOWN" | "VALID";
  };
}

export class WalletRequestAudit {
  readonly #requested = new Set<string>();
  readonly #rejected = new Set<string>();
  #rejectedCount = 0;
  #rejectedUnsafeCount = 0;
  #signedCount = 0;

  recordSigned(method: unknown): void {
    this.#requested.add(auditMethod(method));
    this.#signedCount += 1;
  }

  recordRejected(method: unknown): void {
    const safeMethod = auditMethod(method);
    this.#requested.add(safeMethod);
    this.#rejected.add(safeMethod);
    this.#rejectedCount += 1;
    if (typeof method === "string" && UNSAFE_METHODS.has(method)) {
      this.#rejectedUnsafeCount += 1;
    }
  }

  snapshot(): WalletRequestAuditSnapshot {
    return {
      rejectedMethods: [...this.#rejected].sort(),
      rejectedRequestCount: this.#rejectedCount,
      rejectedUnsafeRequestCount: this.#rejectedUnsafeCount,
      requestedMethods: [...this.#requested].sort(),
      signedMessageCount: this.#signedCount,
    };
  }
}

export function buildApprovedMainnetNamespaces(
  proposal: NamespaceProposal,
  address: string,
): ReturnType<typeof buildApprovedNamespaces> {
  const account = `${MAINNET_CAIP_CHAIN}:${getAddress(address)}`;
  let namespaces: ReturnType<typeof buildApprovedNamespaces>;
  try {
    namespaces = buildApprovedNamespaces({
      proposal,
      supportedNamespaces: {
        eip155: {
          accounts: [account],
          chains: [MAINNET_CAIP_CHAIN],
          events: SAFE_EVENTS,
          methods: [SIWE_METHOD],
        },
      },
    });
  } catch {
    throw new Error("WalletConnect proposal exceeds the permitted scope");
  }
  assertApprovedNamespaces(namespaces, account);
  return namespaces;
}

/** 校验收到的原始 SIWE 字节后直接做 EIP-191 签名，绝不重建待签消息。 */
export async function signValidatedPersonalSignRequest(
  params: unknown,
  signer: MessageSigner,
  expectation: Omit<SiweExpectation, "address">,
): Promise<{ evidence: SanitizedSiweEvidence; signature: string }> {
  const { evidence, messageBytes } = parsePersonalSignRequest(params, {
    ...expectation,
    address: signer.address,
  });
  const signature = await signer.signMessage(messageBytes);
  if (!isHexString(signature, 65)) {
    throw new Error("wallet returned an invalid EIP-191 signature");
  }
  return { evidence, signature };
}

export async function processWalletSessionRequest(
  request: WalletSessionRequest,
  signer: MessageSigner,
  expectation: Omit<SiweExpectation, "address">,
  audit: WalletRequestAudit,
): Promise<WalletSessionRequestResult> {
  const method = request.request.method;
  if (method !== SIWE_METHOD) {
    audit.recordRejected(method);
    return { response: jsonRpcError(request.id, "UNSUPPORTED_METHODS") };
  }
  if (request.chainId !== MAINNET_CAIP_CHAIN) {
    audit.recordRejected(method);
    return { response: jsonRpcError(request.id, "UNSUPPORTED_CHAINS") };
  }
  try {
    const signed = await signValidatedPersonalSignRequest(
      request.request.params,
      signer,
      expectation,
    );
    audit.recordSigned(method);
    return {
      response: jsonRpcResult(request.id, signed.signature),
      siwe: signed.evidence,
    };
  } catch {
    audit.recordRejected(method);
    return { response: jsonRpcError(request.id, "USER_REJECTED") };
  }
}

export function rejectWalletSessionRequest(
  id: number,
  method: unknown,
  audit: WalletRequestAudit,
): WalletSessionRequestResult {
  audit.recordRejected(method);
  return { response: jsonRpcError(id, "USER_REJECTED") };
}

export function buildWalletKitEvidence(
  address: string,
  pairingUriSha256: string,
  siwe: SanitizedSiweEvidence,
  audit: WalletRequestAudit,
): WalletKitEvidence {
  return {
    ...audit.snapshot(),
    address: getAddress(address),
    approvedChains: [MAINNET_CAIP_CHAIN],
    approvedMethods: [SIWE_METHOD],
    chainId: 1,
    pairingUriSha256,
    siwe,
  };
}

export function assertTrustedWalletConnectContext(
  context: WalletConnectVerifyContext,
  expectedOrigin: string,
): void {
  const verified = context.verified;
  if (
    verified.isScam === true ||
    (verified.validation !== "VALID" && verified.validation !== "UNKNOWN") ||
    (verified.validation === "VALID" && verified.origin === "")
  ) {
    throw new Error("WalletConnect Verify rejected the requester");
  }
  if (verified.origin !== "") {
    assertWalletConnectOrigin(verified.origin, expectedOrigin);
  }
}

export function assertWalletConnectOrigin(
  value: string,
  expectedOrigin: string,
): void {
  if (normalizeExpectedOrigin(value) !== expectedOrigin) {
    throw new Error("WalletConnect requester origin is not permitted");
  }
}

export function normalizeExpectedOrigin(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new Error("expected AppKit origin is invalid");
  }
  if (
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username !== "" ||
    url.password !== "" ||
    url.search !== "" ||
    url.hash !== "" ||
    (url.pathname !== "" && url.pathname !== "/")
  ) {
    throw new Error("expected AppKit origin is invalid");
  }
  return url.origin;
}

/** 证据只保存不可逆 SHA-256 十六进制摘要，不保存输入原文。 */
export function hashEvidenceValue(value: string): string {
  return sha256(toUtf8Bytes(value)).slice(2);
}

function parsePersonalSignRequest(
  params: unknown,
  expectation: SiweExpectation,
): { evidence: SanitizedSiweEvidence; messageBytes: Uint8Array } {
  if (!Array.isArray(params) || params.length !== 2) {
    throw new Error("personal_sign parameters are invalid");
  }
  const [first, second] = params;
  if (typeof first !== "string" || typeof second !== "string") {
    throw new Error("personal_sign parameters are invalid");
  }
  const firstIsAddress = sameAddress(first, expectation.address);
  const secondIsAddress = sameAddress(second, expectation.address);
  if (firstIsAddress === secondIsAddress) {
    throw new Error("personal_sign signer is invalid");
  }
  const encodedMessage = firstIsAddress ? second : first;
  if (!isHexString(encodedMessage)) {
    throw new Error("personal_sign message is not AppKit hex data");
  }
  const messageBytes = getBytes(encodedMessage);
  const rawMessage = new TextDecoder("utf-8", { fatal: true }).decode(
    messageBytes,
  );
  const parsed = new SiweMessage(rawMessage);
  const origin = normalizeExpectedOrigin(expectation.expectedOrigin);
  const nowMs = expectation.nowMs ?? Date.now();
  const issuedAt = parsed.issuedAt;
  const expirationTime = parsed.expirationTime;
  if (issuedAt === undefined || expirationTime === undefined) {
    throw new Error("SIWE message is missing its required time window");
  }
  const issuedAtMs = parseRequiredTime(issuedAt, "issuedAt");
  const expirationMs = parseRequiredTime(expirationTime, "expirationTime");
  if (
    parsed.prepareMessage() !== rawMessage ||
    parsed.domain !== new URL(origin).host ||
    parsed.uri !== origin ||
    parsed.version !== "1" ||
    parsed.chainId !== 1 ||
    !sameAddress(parsed.address, expectation.address) ||
    parsed.statement !==
      (expectation.expectedStatement ?? DEFAULT_SIWE_STATEMENT) ||
    !NONCE_PATTERN.test(parsed.nonce) ||
    parsed.scheme !== undefined ||
    parsed.notBefore !== undefined ||
    parsed.requestId !== undefined ||
    parsed.resources !== undefined ||
    issuedAtMs > nowMs + MAX_CLOCK_SKEW_MS ||
    expirationMs <= nowMs ||
    expirationMs <= issuedAtMs ||
    expirationMs - issuedAtMs > MAX_SIWE_LIFETIME_MS
  ) {
    throw new Error("SIWE message does not match the AppKit contract");
  }
  return {
    evidence: {
      address: getAddress(parsed.address),
      chainId: 1,
      domain: parsed.domain,
      expirationTime,
      issuedAt,
      nonceSha256: hashEvidenceValue(parsed.nonce),
      statement: parsed.statement,
      uri: parsed.uri,
      version: "1",
    },
    messageBytes,
  };
}

function assertApprovedNamespaces(
  namespaces: ReturnType<typeof buildApprovedNamespaces>,
  expectedAccount: string,
): void {
  const entries = Object.entries(namespaces);
  if (entries.length === 0) throw new Error("approved namespaces are empty");
  for (const [key, namespace] of entries) {
    const namespaceName = key.split(":", 1)[0];
    if (
      namespaceName !== "eip155" ||
      namespace.methods.length !== 1 ||
      namespace.methods[0] !== SIWE_METHOD ||
      namespace.accounts.length !== 1 ||
      namespace.accounts[0].toLowerCase() !== expectedAccount.toLowerCase() ||
      (namespace.chains?.length ?? 0) !== 1 ||
      namespace.chains?.[0] !== MAINNET_CAIP_CHAIN
    ) {
      throw new Error("approved namespaces exceed the permitted scope");
    }
  }
}

function parseRequiredTime(value: string | undefined, name: string): number {
  const parsed = value === undefined ? Number.NaN : Date.parse(value);
  if (!Number.isFinite(parsed)) throw new Error(`SIWE ${name} is invalid`);
  return parsed;
}

function sameAddress(value: string, expected: string): boolean {
  try {
    return getAddress(value) === getAddress(expected);
  } catch {
    return false;
  }
}

function auditMethod(method: unknown): string {
  return typeof method === "string" && AUDITABLE_METHODS.has(method)
    ? method
    : UNKNOWN_METHOD;
}

function jsonRpcResult(id: number, result: string): WalletJsonRpcResponse {
  return { id, jsonrpc: "2.0", result };
}

function jsonRpcError(
  id: number,
  key: "UNSUPPORTED_CHAINS" | "UNSUPPORTED_METHODS" | "USER_REJECTED",
): WalletJsonRpcResponse {
  return { error: getSdkError(key), id, jsonrpc: "2.0" };
}
