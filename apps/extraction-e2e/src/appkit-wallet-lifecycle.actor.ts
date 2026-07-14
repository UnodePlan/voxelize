import { describe, expect, it, vi } from "vitest";

import {
  SIWE_METHOD,
  WalletRequestAudit,
  rejectWalletSessionRequest,
  type SanitizedSiweEvidence,
} from "./live/walletkit-validation";
import {
  runBoundedWalletKitCleanup,
  WalletKitEvidenceTracker,
  WalletKitRequestCoordinator,
  type WalletKitCleanupPhase,
} from "./live/walletkit-lifecycle";

const ADDRESS = "0x1111111111111111111111111111111111111111";
const PAIRING_HASH = "a".repeat(64);
const SIWE_EVIDENCE: SanitizedSiweEvidence = {
  address: ADDRESS,
  chainId: 1,
  domain: "game.example.test",
  expirationTime: "2030-01-01T00:05:00.000Z",
  issuedAt: "2030-01-01T00:00:00.000Z",
  nonceSha256: "b".repeat(64),
  statement: "Sign in to Voxel Extraction.",
  uri: "https://game.example.test",
  version: "1",
};

describe("WalletKit lifecycle coordinator", () => {
  it("rebuilds final evidence after a late request has drained", async () => {
    const audit = new WalletRequestAudit();
    const evidence = completedEvidenceTracker(audit);
    audit.recordSigned(SIWE_METHOD);
    expect(evidence.evidence().requestedMethods).toEqual([SIWE_METHOD]);

    const coordinator = new WalletKitRequestCoordinator();
    const blocker = deferred<void>();
    const lateRequest = coordinator.run(
      "eth_sendTransaction",
      async () => {
        await blocker.promise;
        return rejectWalletSessionRequest(2, "eth_sendTransaction", audit);
      },
      async () => rejectWalletSessionRequest(2, "eth_sendTransaction", audit),
    );

    coordinator.stopAccepting();
    const drain = coordinator.drain();
    blocker.resolve();
    await Promise.all([lateRequest, drain]);

    expect(evidence.evidence()).toMatchObject({
      rejectedMethods: ["eth_sendTransaction"],
      rejectedRequestCount: 1,
      rejectedUnsafeRequestCount: 1,
      requestedMethods: [SIWE_METHOD, "eth_sendTransaction"].sort(),
      signedMessageCount: 1,
    });
  });

  it("serializes handlers and allows only one concurrent personal_sign", async () => {
    const coordinator = new WalletKitRequestCoordinator();
    const blocker = deferred<void>();
    const order: string[] = [];
    let signCount = 0;
    let rejectionCount = 0;

    const first = coordinator.run(
      SIWE_METHOD,
      async () => {
        signCount += 1;
        order.push("first:start");
        await blocker.promise;
        order.push("first:end");
        return "signed";
      },
      async () => "unexpected rejection",
    );
    const second = coordinator.run(
      SIWE_METHOD,
      async () => {
        signCount += 1;
        return "unexpected signature";
      },
      async () => {
        rejectionCount += 1;
        order.push("second:rejected");
        return "rejected";
      },
    );

    await Promise.resolve();
    expect(signCount).toBe(1);
    expect(order).toEqual(["first:start"]);
    blocker.resolve();

    await expect(first).resolves.toBe("signed");
    await expect(second).resolves.toBe("rejected");
    expect(signCount).toBe(1);
    expect(rejectionCount).toBe(1);
    expect(order).toEqual(["first:start", "first:end", "second:rejected"]);
  });

  it("drains accepted handlers before session, pairing, and relay cleanup", async () => {
    const coordinator = new WalletKitRequestCoordinator();
    const blocker = deferred<void>();
    const order: string[] = [];
    const request = coordinator.run(
      "eth_sendTransaction",
      async () => {
        order.push("request:start");
        await blocker.promise;
        order.push("request:end");
      },
      async () => undefined,
    );
    coordinator.stopAccepting();

    const cleanup = runBoundedWalletKitCleanup(
      cleanupPhases(coordinator, order),
      100,
    );
    await Promise.resolve();
    expect(order).toEqual(["request:start"]);

    blocker.resolve();
    await Promise.all([request, cleanup]);
    expect(order).toEqual([
      "request:start",
      "request:end",
      "session",
      "pairing",
      "relay",
    ]);
  });

  it("bounds every cleanup operation, aggregates failures, and continues", async () => {
    const relay = vi.fn(async () => undefined);
    const phases: WalletKitCleanupPhase[] = [
      { name: "request-drain", operations: () => [] },
      {
        name: "sessions",
        operations: () => [() => new Promise<void>(() => undefined)],
      },
      {
        name: "pairings",
        operations: () => [async () => Promise.reject(new Error("sensitive"))],
      },
      { name: "relay", operations: () => [relay] },
    ];

    let failure: unknown;
    try {
      await runBoundedWalletKitCleanup(phases, 10);
    } catch (error) {
      failure = error;
    }

    expect(failure).toBeInstanceOf(AggregateError);
    const aggregate = failure as AggregateError;
    expect(aggregate.errors).toHaveLength(2);
    expect(JSON.stringify(aggregate.errors)).not.toContain("sensitive");
    expect(relay).toHaveBeenCalledOnce();
  });
});

function completedEvidenceTracker(
  audit: WalletRequestAudit,
): WalletKitEvidenceTracker {
  const tracker = new WalletKitEvidenceTracker(ADDRESS, audit);
  tracker.setPairingUriSha256(PAIRING_HASH);
  tracker.completeSiwe(SIWE_EVIDENCE);
  return tracker;
}

function cleanupPhases(
  coordinator: WalletKitRequestCoordinator,
  order: string[],
): WalletKitCleanupPhase[] {
  const phase = (
    name: "sessions" | "pairings" | "relay",
    marker: string,
  ): WalletKitCleanupPhase => ({
    name,
    operations: () => [async () => void order.push(marker)],
  });
  return [
    {
      name: "request-drain",
      operations: () => [() => coordinator.drain()],
    },
    phase("sessions", "session"),
    phase("pairings", "pairing"),
    phase("relay", "relay"),
  ];
}

function deferred<T>(): { promise: Promise<T>; resolve(value: T): void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}
