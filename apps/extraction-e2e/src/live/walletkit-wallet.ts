import { WalletKit, type WalletKitTypes } from "@reown/walletkit";
import { Core } from "@walletconnect/core";
import { getSdkError } from "@walletconnect/utils";
import { Wallet } from "ethers";

import {
  assertTrustedWalletConnectContext,
  assertWalletConnectOrigin,
  buildApprovedMainnetNamespaces,
  DEFAULT_SIWE_STATEMENT,
  hashEvidenceValue,
  normalizeExpectedOrigin,
  processWalletSessionRequest,
  rejectWalletSessionRequest,
  SIWE_METHOD,
  WalletRequestAudit,
  type WalletKitEvidence,
  type WalletSessionRequestResult,
} from "./walletkit-validation";
import {
  createWalletKitDeferred,
  runBoundedWalletKitCleanup,
  WalletKitEvidenceTracker,
  WalletKitRequestCoordinator,
  withWalletKitTimeout,
} from "./walletkit-lifecycle";

export type { WalletKitEvidence } from "./walletkit-validation";

const DEFAULT_TIMEOUT_MS = 120_000;
const MAX_TIMEOUT_MS = 10 * 60_000;
export interface WalletKitAcceptanceWalletOptions {
  expectedOrigin: string;
  expectedStatement?: string;
  projectId: string;
  timeoutMs?: number;
}

export interface WalletKitAcceptanceWallet {
  address: string;
  evidence(): WalletKitEvidence;
  pair(uri: string): Promise<void>;
  stop(): Promise<void>;
  waitForSiwe(): Promise<WalletKitEvidence>;
}

/** WalletConnect 默认存储可能写磁盘；验收钱包必须把所有协议状态限制在进程内。 */
export class InMemoryKeyValueStorage {
  readonly #values = new Map<string, unknown>();

  async getKeys(): Promise<string[]> {
    return [...this.#values.keys()];
  }

  async getEntries<T = unknown>(): Promise<[string, T][]> {
    return [...this.#values.entries()] as [string, T][];
  }

  async getItem<T = unknown>(key: string): Promise<T | undefined> {
    return this.#values.get(key) as T | undefined;
  }

  async setItem<T = unknown>(key: string, value: T): Promise<void> {
    this.#values.set(key, value);
  }

  async removeItem(key: string): Promise<void> {
    this.#values.delete(key);
  }
}

export async function createWalletKitAcceptanceWallet({
  expectedOrigin,
  expectedStatement = DEFAULT_SIWE_STATEMENT,
  projectId,
  timeoutMs = DEFAULT_TIMEOUT_MS,
}: WalletKitAcceptanceWalletOptions): Promise<WalletKitAcceptanceWallet> {
  const origin = normalizeExpectedOrigin(expectedOrigin);
  if (projectId.trim() === "")
    throw new Error("WalletKit project ID is required");
  if (!isPermittedTimeout(timeoutMs)) {
    throw new Error("WalletKit timeout is outside the permitted range");
  }
  const cleanupTimeoutMs = Math.min(timeoutMs, 10_000);

  const signer = Wallet.createRandom();
  const core = new Core({
    customStoragePrefix: `extraction-e2e-${signer.address.toLowerCase()}`,
    logger: "silent",
    projectId: projectId.trim(),
    storage: new InMemoryKeyValueStorage(),
    telemetryEnabled: false,
  });
  let walletKit: Awaited<ReturnType<typeof WalletKit.init>>;
  try {
    walletKit = await WalletKit.init({
      core,
      metadata: {
        description: "Protocol-only acceptance wallet",
        icons: [],
        name: "Voxel Extraction E2E Wallet",
        url: origin,
      },
    });
  } catch {
    try {
      if (core.relayer.connected) {
        await withWalletKitTimeout(
          core.relayer.transportClose(),
          cleanupTimeoutMs,
          "WalletKit initialization cleanup timed out",
        );
      }
    } catch {
      // 初始化错误始终脱敏；清理失败不能覆盖主错误或泄露 Relay 参数。
    }
    throw new Error("WalletKit initialization failed");
  }
  const audit = new WalletRequestAudit();
  const evidenceTracker = new WalletKitEvidenceTracker(signer.address, audit);
  const paired = createWalletKitDeferred<void>();
  const requestCoordinator = new WalletKitRequestCoordinator();
  const siweCompleted = createWalletKitDeferred<WalletKitEvidence>();
  let pairingStarted = false;
  let stopPromise: Promise<void> | undefined;
  let stopped = false;

  // 故意不监听 session_authenticate：AppKit 会按标准回退到 session_proposal，
  // 随后的 SIWX 页面仍须由浏览器点击 Sign 才会发出真实 personal_sign。
  const onProposal = (event: WalletKitTypes.SessionProposal): void => {
    void (async () => {
      try {
        if (stopped || !pairingStarted || paired.settled()) {
          throw new Error("unexpected WalletConnect proposal");
        }
        assertTrustedWalletConnectContext(event.verifyContext, origin);
        assertWalletConnectOrigin(event.params.proposer.metadata.url, origin);
        const namespaces = buildApprovedMainnetNamespaces(
          event.params,
          signer.address,
        );
        await walletKit.approveSession({ id: event.id, namespaces });
        paired.resolve();
      } catch {
        await walletKit
          .rejectSession({ id: event.id, reason: getSdkError("USER_REJECTED") })
          .catch(() => undefined);
        paired.reject(new Error("WalletConnect proposal was rejected"));
      }
    })().catch(() => {
      paired.reject(new Error("WalletConnect proposal handling failed"));
    });
  };

  const respondToRequest = async (
    event: WalletKitTypes.SessionRequest,
    result: WalletSessionRequestResult,
  ): Promise<void> => {
    await walletKit.respondSessionRequest({
      response: result.response,
      topic: event.topic,
    });
    if (result.siwe !== undefined) {
      evidenceTracker.completeSiwe(result.siwe);
      siweCompleted.resolve(evidenceTracker.evidence());
    } else if (event.params.request.method === SIWE_METHOD) {
      siweCompleted.reject(new Error("SIWE personal_sign was rejected"));
    }
  };

  const handleRequest = async (
    event: WalletKitTypes.SessionRequest,
  ): Promise<void> => {
    let result: WalletSessionRequestResult;
    try {
      assertTrustedWalletConnectContext(event.verifyContext, origin);
      if (!walletKit.getActiveSessions()[event.topic]) {
        throw new Error("WalletConnect session is not active");
      }
      result = await processWalletSessionRequest(
        { id: event.id, ...event.params },
        signer,
        { expectedOrigin: origin, expectedStatement },
        audit,
      );
    } catch {
      result = rejectWalletSessionRequest(
        event.id,
        event.params.request.method,
        audit,
      );
    }
    await respondToRequest(event, result);
  };

  const rejectRequest = (event: WalletKitTypes.SessionRequest): Promise<void> =>
    respondToRequest(
      event,
      rejectWalletSessionRequest(event.id, event.params.request.method, audit),
    );

  const onRequest = (event: WalletKitTypes.SessionRequest): void => {
    void requestCoordinator
      .run(
        event.params.request.method,
        () => handleRequest(event),
        () => rejectRequest(event),
      )
      .catch(() => {
        siweCompleted.reject(
          new Error("WalletConnect request handling failed"),
        );
      });
  };

  walletKit.on("session_proposal", onProposal);
  walletKit.on("session_request", onRequest);

  return {
    address: signer.address,
    evidence: () => evidenceTracker.evidence(),
    pair: async (uri) => {
      if (pairingStarted || stopped || !uri.startsWith("wc:")) {
        throw new Error("WalletConnect pairing request is invalid");
      }
      pairingStarted = true;
      evidenceTracker.setPairingUriSha256(hashEvidenceValue(uri));
      try {
        await walletKit.pair({ uri });
      } catch {
        throw new Error("WalletConnect pairing failed");
      }
      await withWalletKitTimeout(
        paired.promise,
        timeoutMs,
        "WalletConnect proposal timed out",
      );
    },
    stop: () => {
      if (stopPromise !== undefined) return stopPromise;
      stopped = true;
      walletKit.off("session_proposal", onProposal);
      walletKit.off("session_request", onRequest);
      requestCoordinator.stopAccepting();
      paired.reject(new Error("WalletKit acceptance wallet stopped"));
      siweCompleted.reject(new Error("WalletKit acceptance wallet stopped"));
      stopPromise = runBoundedWalletKitCleanup(
        [
          {
            name: "request-drain",
            operations: () => [() => requestCoordinator.drain()],
          },
          {
            name: "sessions",
            operations: () =>
              Object.keys(walletKit.getActiveSessions()).map(
                (topic) => () =>
                  walletKit.disconnectSession({
                    reason: getSdkError("USER_DISCONNECTED"),
                    topic,
                  }),
              ),
          },
          {
            name: "pairings",
            operations: () =>
              core.pairing
                .getPairings()
                .map(
                  (pairing) => () =>
                    core.pairing.disconnect({ topic: pairing.topic }),
                ),
          },
          {
            name: "relay",
            operations: () =>
              core.relayer.connected
                ? [() => core.relayer.transportClose()]
                : [],
          },
        ],
        cleanupTimeoutMs,
      );
      return stopPromise;
    },
    waitForSiwe: () =>
      withWalletKitTimeout(
        siweCompleted.promise,
        timeoutMs,
        "WalletConnect SIWE request timed out",
      ),
  };
}

function isPermittedTimeout(value: number): boolean {
  return Number.isSafeInteger(value) && value > 0 && value <= MAX_TIMEOUT_MS;
}
