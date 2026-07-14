import {
  getBytes,
  hexlify,
  sha256,
  toUtf8Bytes,
  verifyMessage,
  Wallet,
} from "ethers";
import { SiweMessage } from "siwe";
import { describe, expect, it, vi } from "vitest";

import {
  buildApprovedMainnetNamespaces,
  DEFAULT_SIWE_STATEMENT,
  MAINNET_CAIP_CHAIN,
  processWalletSessionRequest,
  signValidatedPersonalSignRequest,
  SIWE_METHOD,
  WalletRequestAudit,
} from "./live/walletkit-validation";
import { InMemoryKeyValueStorage } from "./live/walletkit-wallet";

const ORIGIN = "https://game.example.test";
const NOW_MS = Date.parse("2030-01-01T00:00:00.000Z");

describe("WalletKit AppKit acceptance wallet", () => {
  it("keeps WalletConnect state in isolated process memory", async () => {
    const first = new InMemoryKeyValueStorage();
    await first.setItem("session", { topic: "memory-only" });
    expect(await first.getKeys()).toEqual(["session"]);
    expect(await first.getEntries()).toEqual([
      ["session", { topic: "memory-only" }],
    ]);

    const second = new InMemoryKeyValueStorage();
    expect(await second.getItem("session")).toBeUndefined();
    await first.removeItem("session");
    expect(await first.getKeys()).toEqual([]);
  });

  it("intersects an AppKit proposal with Mainnet personal_sign only", () => {
    const wallet = Wallet.createRandom();
    const namespaces = buildApprovedMainnetNamespaces(
      proposal(
        [],
        [SIWE_METHOD, "eth_signTypedData_v4", "eth_sendTransaction"],
      ),
      wallet.address,
    );

    expect(Object.keys(namespaces)).toEqual(["eip155"]);
    expect(namespaces.eip155).toMatchObject({
      accounts: [`${MAINNET_CAIP_CHAIN}:${wallet.address}`],
      chains: [MAINNET_CAIP_CHAIN],
      methods: [SIWE_METHOD],
    });
  });

  it("rejects a proposal that requires transaction authority", () => {
    expect(() =>
      buildApprovedMainnetNamespaces(
        proposal(["eth_sendTransaction"], [SIWE_METHOD]),
        Wallet.createRandom().address,
      ),
    ).toThrow(/permitted scope/u);
  });

  it("signs the exact AppKit SIWE bytes with EIP-191 and keeps evidence redacted", async () => {
    const wallet = Wallet.createRandom();
    const nonce = randomNonce();
    const message = createSiweMessage(wallet.address, nonce);
    const encoded = hexlify(toUtf8Bytes(message));
    const audit = new WalletRequestAudit();

    const result = await processWalletSessionRequest(
      {
        chainId: MAINNET_CAIP_CHAIN,
        id: 7,
        request: {
          method: SIWE_METHOD,
          params: [encoded, wallet.address.toLowerCase()],
        },
      },
      wallet,
      {
        expectedOrigin: ORIGIN,
        expectedStatement: DEFAULT_SIWE_STATEMENT,
        nowMs: NOW_MS,
      },
      audit,
    );

    if (!("result" in result.response) || result.siwe === undefined) {
      throw new Error("expected a successful personal_sign response");
    }
    expect(
      verifyMessage(getBytes(encoded), result.response.result).toLowerCase(),
    ).toBe(wallet.address.toLowerCase());
    expect(result.siwe).toMatchObject({
      address: wallet.address,
      chainId: 1,
      domain: "game.example.test",
      statement: DEFAULT_SIWE_STATEMENT,
      uri: ORIGIN,
      version: "1",
    });
    expect(result.siwe.nonceSha256).toBe(sha256(toUtf8Bytes(nonce)).slice(2));
    expect(audit.snapshot()).toEqual({
      rejectedMethods: [],
      rejectedRequestCount: 0,
      rejectedUnsafeRequestCount: 0,
      requestedMethods: [SIWE_METHOD],
      signedMessageCount: 1,
    });
    const safeEvidence = JSON.stringify(result.siwe);
    expect(safeEvidence).not.toContain(nonce);
    expect(safeEvidence).not.toContain(message);
    expect(safeEvidence).not.toContain(result.response.result);
  });

  it.each([
    ["domain", { domain: "attacker.example.test" }],
    ["URI", { uri: "https://attacker.example.test" }],
    ["chain", { chainId: 10 }],
    ["statement", { statement: "Approve everything" }],
    ["lifetime", { expirationTime: "2030-01-01T00:05:01.000Z" }],
    ["expiry", { expirationTime: "2029-12-31T23:59:59.000Z" }],
  ])("rejects a SIWE message with a mismatched %s", async (_, overrides) => {
    const wallet = Wallet.createRandom();
    const message = createSiweMessage(wallet.address, randomNonce(), overrides);
    await expect(
      signValidatedPersonalSignRequest(
        [hexlify(toUtf8Bytes(message)), wallet.address],
        wallet,
        {
          expectedOrigin: ORIGIN,
          expectedStatement: DEFAULT_SIWE_STATEMENT,
          nowMs: NOW_MS,
        },
      ),
    ).rejects.toThrow();
  });

  it("rejects plaintext, the wrong account, and the wrong chain", async () => {
    const wallet = Wallet.createRandom();
    const message = createSiweMessage(wallet.address, randomNonce());
    const expectation = {
      expectedOrigin: ORIGIN,
      expectedStatement: DEFAULT_SIWE_STATEMENT,
      nowMs: NOW_MS,
    };
    await expect(
      signValidatedPersonalSignRequest(
        [message, wallet.address],
        wallet,
        expectation,
      ),
    ).rejects.toThrow();
    await expect(
      signValidatedPersonalSignRequest(
        [hexlify(toUtf8Bytes(message)), Wallet.createRandom().address],
        wallet,
        expectation,
      ),
    ).rejects.toThrow();

    const audit = new WalletRequestAudit();
    const result = await processWalletSessionRequest(
      {
        chainId: "eip155:10",
        id: 9,
        request: {
          method: SIWE_METHOD,
          params: [hexlify(toUtf8Bytes(message)), wallet.address],
        },
      },
      wallet,
      expectation,
      audit,
    );
    expect("error" in result.response).toBe(true);
    expect(audit.snapshot().signedMessageCount).toBe(0);
  });

  it("rejects and audits transaction, permission, typed-sign, and unknown requests", async () => {
    const signer = {
      address: Wallet.createRandom().address,
      signMessage: vi.fn(async () => "unreachable"),
    };
    const audit = new WalletRequestAudit();
    for (const method of [
      "eth_sendTransaction",
      "wallet_grantPermissions",
      "eth_signTypedData_v4",
      "attacker-controlled-method",
    ]) {
      const result = await processWalletSessionRequest(
        {
          chainId: MAINNET_CAIP_CHAIN,
          id: 11,
          request: { method, params: [] },
        },
        signer,
        { expectedOrigin: ORIGIN, nowMs: NOW_MS },
        audit,
      );
      expect("error" in result.response).toBe(true);
    }

    expect(signer.signMessage).not.toHaveBeenCalled();
    expect(audit.snapshot()).toEqual({
      rejectedMethods: [
        "eth_sendTransaction",
        "eth_signTypedData_v4",
        "unknown_method",
        "wallet_grantPermissions",
      ],
      rejectedRequestCount: 4,
      rejectedUnsafeRequestCount: 2,
      requestedMethods: [
        "eth_sendTransaction",
        "eth_signTypedData_v4",
        "unknown_method",
        "wallet_grantPermissions",
      ],
      signedMessageCount: 0,
    });
  });
});

function createSiweMessage(
  address: string,
  nonce: string,
  overrides: Partial<SiweMessage> = {},
): string {
  return new SiweMessage({
    address,
    chainId: 1,
    domain: "game.example.test",
    expirationTime: "2030-01-01T00:05:00.000Z",
    issuedAt: "2030-01-01T00:00:00.000Z",
    nonce,
    statement: DEFAULT_SIWE_STATEMENT,
    uri: ORIGIN,
    version: "1",
    ...overrides,
  }).prepareMessage();
}

function randomNonce(): string {
  return [...crypto.getRandomValues(new Uint8Array(16))]
    .map((value) => value.toString(16).padStart(2, "0"))
    .join("");
}

function proposal(
  requiredMethods: string[],
  optionalMethods: string[],
): Parameters<typeof buildApprovedMainnetNamespaces>[0] {
  return {
    expiryTimestamp: 2_000_000_000,
    id: 1,
    optionalNamespaces: {
      eip155: {
        chains: [MAINNET_CAIP_CHAIN],
        events: ["accountsChanged", "chainChanged"],
        methods: optionalMethods,
      },
    },
    pairingTopic: "memory-only-pairing",
    proposer: {
      metadata: {
        description: "test proposer",
        icons: [],
        name: "test proposer",
        url: ORIGIN,
      },
      publicKey: "test-public-key",
    },
    relays: [{ protocol: "irn" }],
    requiredNamespaces:
      requiredMethods.length === 0
        ? {}
        : {
            eip155: {
              chains: [MAINNET_CAIP_CHAIN],
              events: [],
              methods: requiredMethods,
            },
          },
  };
}
