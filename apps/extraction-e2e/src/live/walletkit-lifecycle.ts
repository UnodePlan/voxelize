import {
  buildWalletKitEvidence,
  SIWE_METHOD,
  WalletRequestAudit,
  type SanitizedSiweEvidence,
  type WalletKitEvidence,
} from "./walletkit-validation";

const MAX_CLEANUP_OPERATION_TIMEOUT_MS = 10_000;

/** 首签只记录脱敏 SIWE；每次读取都结合当前审计生成新证据。 */
export class WalletKitEvidenceTracker {
  readonly #address: string;
  readonly #audit: WalletRequestAudit;
  #pairingUriSha256: string | undefined;
  #siwe: SanitizedSiweEvidence | undefined;

  constructor(address: string, audit: WalletRequestAudit) {
    this.#address = address;
    this.#audit = audit;
  }

  setPairingUriSha256(value: string): void {
    if (
      this.#pairingUriSha256 !== undefined ||
      !/^[0-9a-f]{64}$/u.test(value)
    ) {
      throw new Error("WalletConnect pairing evidence is invalid");
    }
    this.#pairingUriSha256 = value;
  }

  completeSiwe(siwe: SanitizedSiweEvidence): void {
    if (this.#siwe !== undefined) {
      throw new Error("WalletConnect SIWE evidence is already complete");
    }
    this.#siwe = Object.freeze({ ...siwe });
  }

  evidence(): WalletKitEvidence {
    if (this.#pairingUriSha256 === undefined || this.#siwe === undefined) {
      throw new Error("WalletConnect SIWE evidence is not complete");
    }
    return buildWalletKitEvidence(
      this.#address,
      this.#pairingUriSha256,
      { ...this.#siwe },
      this.#audit,
    );
  }
}

/** 串行执行请求，并在入队时原子占用唯一的 personal_sign 资格。 */
export class WalletKitRequestCoordinator {
  #accepting = true;
  readonly #inFlight = new Set<Promise<unknown>>();
  #personalSignClaimed = false;
  #tail: Promise<void> = Promise.resolve();

  run<T>(
    method: unknown,
    operation: () => Promise<T>,
    rejectOperation: () => Promise<T>,
  ): Promise<T> {
    const duplicateSign = method === SIWE_METHOD && this.#personalSignClaimed;
    const reject = !this.#accepting || duplicateSign;
    if (!reject && method === SIWE_METHOD) this.#personalSignClaimed = true;

    const pending = this.#tail.then(reject ? rejectOperation : operation);
    this.#tail = pending.then(
      () => undefined,
      () => undefined,
    );
    this.#inFlight.add(pending);
    void pending.then(
      () => this.#inFlight.delete(pending),
      () => this.#inFlight.delete(pending),
    );
    return pending;
  }

  stopAccepting(): void {
    this.#accepting = false;
  }

  async drain(): Promise<void> {
    await Promise.allSettled([...this.#inFlight]);
  }
}

export type WalletKitCleanupPhaseName =
  | "request-drain"
  | "sessions"
  | "pairings"
  | "relay";

export interface WalletKitCleanupPhase {
  name: WalletKitCleanupPhaseName;
  operations(): readonly (() => Promise<void>)[];
}

/** 各阶段顺序执行，阶段内并发；每个操作独立受同一超时上限保护。 */
export async function runBoundedWalletKitCleanup(
  phases: readonly WalletKitCleanupPhase[],
  timeoutMs: number,
): Promise<void> {
  if (
    !Number.isSafeInteger(timeoutMs) ||
    timeoutMs <= 0 ||
    timeoutMs > MAX_CLEANUP_OPERATION_TIMEOUT_MS
  ) {
    throw new Error("WalletKit cleanup timeout is outside the permitted range");
  }
  const failures: Error[] = [];
  for (const phase of phases) {
    let operations: readonly (() => Promise<void>)[];
    try {
      operations = phase.operations();
    } catch {
      failures.push(new Error(`WalletKit ${phase.name} cleanup failed`));
      continue;
    }
    const results = await Promise.all(
      operations.map(async (operation) => {
        try {
          await withWalletKitTimeout(
            Promise.resolve().then(operation),
            timeoutMs,
            `WalletKit ${phase.name} cleanup timed out`,
          );
          return undefined;
        } catch {
          return new Error(`WalletKit ${phase.name} cleanup failed`);
        }
      }),
    );
    failures.push(...results.filter((result) => result !== undefined));
  }
  if (failures.length > 0) {
    throw new AggregateError(
      failures,
      "WalletKit acceptance wallet cleanup failed",
    );
  }
}

export interface WalletKitDeferred<T> {
  promise: Promise<T>;
  reject(reason: Error): void;
  resolve(value: T): void;
  settled(): boolean;
}

export function createWalletKitDeferred<T>(): WalletKitDeferred<T> {
  let isSettled = false;
  let resolvePromise!: (value: T) => void;
  let rejectPromise!: (reason: Error) => void;
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve;
    rejectPromise = reject;
  });
  void promise.catch(() => undefined);
  return {
    promise,
    reject: (reason) => {
      if (isSettled) return;
      isSettled = true;
      rejectPromise(reason);
    },
    resolve: (value) => {
      if (isSettled) return;
      isSettled = true;
      resolvePromise(value);
    },
    settled: () => isSettled,
  };
}

export async function withWalletKitTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}
