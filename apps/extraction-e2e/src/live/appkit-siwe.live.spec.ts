import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import {
  expect,
  test,
  type Page,
  type Response,
  type TestInfo,
} from "@playwright/test";

import {
  assertOnlyKeys,
  readNonEmptyString,
  readRecord,
} from "../../../../contracts/extraction/v1/decoder-utils";
import clientPackage from "../../../extraction-client/package.json" with {
  type: "json",
};
import {
  decodeQueueSnapshot,
  decodeWarehouse,
} from "../../../extraction-client/src/api/game-decoders";
import type {
  QueueSnapshot,
  WarehouseSnapshot,
} from "../../../extraction-client/src/api/models";
import e2ePackage from "../../package.json" with { type: "json" };

import {
  approveProductSiwe,
  attachSafeScreenshot,
  openProductWalletConnect,
} from "./appkit-modal";
import { loadLiveE2eConfig } from "./config";
import { hashEvidenceValue } from "./walletkit-validation";
import { createWalletKitAcceptanceWallet } from "./walletkit-wallet";

const ETHEREUM_MAINNET_CHAIN_ID = 1;
const EXPECTED_STATEMENT = "Sign in to Voxel Extraction.";
const EVIDENCE_SCHEMA = "extraction.e2e.appkit-siwe.v1";
const TRANSACTION_METHODS = new Set([
  "eth_sendRawTransaction",
  "eth_sendTransaction",
  "eth_signTransaction",
  "wallet_sendCalls",
]);
const GATE_SOURCE_PATHS = [
  "apps/extraction-client/package.json",
  "apps/extraction-client/src/auth/appkit.ts",
  "apps/extraction-e2e/package.json",
  "apps/extraction-e2e/playwright.config.ts",
  "apps/extraction-e2e/scripts/live-smoke-config.mjs",
  "apps/extraction-e2e/scripts/live-smoke-orchestration.mjs",
  "apps/extraction-e2e/scripts/run-live-smoke.mjs",
  "apps/extraction-e2e/src/live/appkit-modal.ts",
  "apps/extraction-e2e/src/live/appkit-siwe.live.spec.ts",
  "apps/extraction-e2e/src/live/config.ts",
  "apps/extraction-e2e/src/live/walletkit-validation.ts",
  "apps/extraction-e2e/src/live/walletkit-wallet.ts",
  "pnpm-lock.yaml",
] as const;

test.describe.configure({ mode: "serial" });
// 失败时的自动截图或 trace 可能保存仍在二维码 DOM 中的配对密钥。
test.use({ screenshot: "off", trace: "off", video: "off" });

test("产品 AppKit 经真实 WalletKit 完成 Mainnet SIWE @appkit", async ({
  page,
}, testInfo) => {
  const config = loadLiveE2eConfig();
  const projectId = requireProjectId(process.env.VITE_REOWN_PROJECT_ID);
  const wallet = await createWalletKitAcceptanceWallet({
    expectedOrigin: config.publicOrigin,
    expectedStatement: EXPECTED_STATEMENT,
    projectId,
    timeoutMs: config.scenarioTimeoutMs,
  });
  const http = new ProductHttpWitness(
    page,
    config.clientUrl,
    wallet.address,
    projectId,
  );
  let primaryError: Error | null = null;
  let cleanupError: Error | null = null;

  try {
    await page.goto(config.clientUrl, {
      timeout: config.scenarioTimeoutMs,
      waitUntil: "domcontentloaded",
    });

    let pairingUri: string | null = await openProductWalletConnect(
      page,
      testInfo,
      config.scenarioTimeoutMs,
    );
    try {
      await wallet.pair(pairingUri);
    } finally {
      pairingUri = null;
    }

    const walletSiwe = wallet.waitForSiwe();
    const authenticatedHttp = http.waitForAuthentication(
      config.scenarioTimeoutMs,
    );
    await approveProductSiwe(page, testInfo, config.scenarioTimeoutMs);

    const [, authEvidence] = await Promise.all([
      walletSiwe,
      authenticatedHttp,
      waitForLobby(page, config.scenarioTimeoutMs),
    ]);
    await assertWarehouseRendered(page);
    await attachSafeScreenshot(testInfo, page, "appkit-authenticated-lobby");

    const joinedHttp = http.waitForQueueJoin(config.scenarioTimeoutMs);
    await page.locator('[data-action="join-queue"]').click({
      timeout: config.scenarioTimeoutMs,
    });
    const queueJoined = await joinedHttp;
    expect(queueJoined.queue.status).toBe("queued");
    await page
      .locator('[data-product-shell][data-screen="queue"]')
      .waitFor({ state: "attached", timeout: config.scenarioTimeoutMs });
    await attachSafeScreenshot(testInfo, page, "appkit-queue");

    const leftHttp = http.waitForQueueLeave(config.scenarioTimeoutMs);
    await page.locator('[data-action="leave-queue"]').click({
      timeout: config.scenarioTimeoutMs,
    });
    const queueLeft = await leftHttp;
    expect(queueLeft.queue.status).toBe("idle");
    expect(queueLeft.queue.removed).toBe(true);
    await waitForLobby(page, config.scenarioTimeoutMs);

    // 先停止接纳并排空钱包请求，再读取最终审计，避免首签后的迟到请求漏报。
    await wallet.stop();
    const walletEvidence = wallet.evidence();
    const transactionRequestsObserved = countTransactionRequests(
      walletEvidence.requestedMethods,
    );
    expect(walletEvidence.address.toLowerCase()).toBe(
      wallet.address.toLowerCase(),
    );
    expect(walletEvidence.chainId).toBe(ETHEREUM_MAINNET_CHAIN_ID);
    expect(walletEvidence.approvedChains).toEqual(["eip155:1"]);
    expect(walletEvidence.approvedMethods).toEqual(["personal_sign"]);
    expect(walletEvidence.requestedMethods).toEqual(["personal_sign"]);
    expect(walletEvidence.signedMessageCount).toBe(1);
    expect(walletEvidence.rejectedRequestCount).toBe(0);
    expect(walletEvidence.rejectedUnsafeRequestCount).toBe(0);
    expect(transactionRequestsObserved).toBe(0);

    expect(authEvidence.nonce.nonceSha256).toBe(
      walletEvidence.siwe.nonceSha256,
    );

    const evidence = {
      schema: EVIDENCE_SCHEMA,
      createdAt: new Date().toISOString(),
      origin: config.publicOrigin,
      network: "eip155:1",
      transport: "WalletConnect production Relay",
      source: sourceEvidence(),
      versions: {
        appKit: clientPackage.dependencies["@reown/appkit"],
        walletConnectCore: e2ePackage.devDependencies["@walletconnect/core"],
        walletKit: e2ePackage.devDependencies["@reown/walletkit"],
      },
      productUi: {
        appKitWalletSelection: true,
        appKitSiweConsent: true,
        pairingUriPersisted: false,
        screenshots: [
          "appkit-wallet-selection",
          "appkit-siwe-consent",
          "appkit-authenticated-lobby",
          "appkit-queue",
        ],
      },
      wallet: walletEvidence,
      http: {
        ...authEvidence,
        queueJoined,
        queueLeft,
      },
      transactionSafety: {
        requestedMethods: walletEvidence.requestedMethods,
        rejectedUnsafeRequestCount: walletEvidence.rejectedUnsafeRequestCount,
        transactionRequestsObserved,
      },
    };
    assertSecretFreeEvidence(evidence, projectId);
    await saveEvidence(config.artifactDir, testInfo, evidence);
  } catch (error) {
    primaryError = redactError(error, projectId);
  } finally {
    http.dispose();
    try {
      await wallet.stop();
    } catch (error) {
      cleanupError = redactError(error, projectId);
    }
  }

  if (primaryError !== null && cleanupError !== null) {
    throw new AggregateError(
      [primaryError, cleanupError],
      primaryError.message,
    );
  }
  if (primaryError !== null) throw primaryError;
  if (cleanupError !== null) throw cleanupError;
});

interface HttpStep {
  method: string;
  observedAt: string;
  path: string;
  status: number;
}

interface SessionStep extends HttpStep {
  address: string;
  chainId: 1;
}

interface NonceStep extends HttpStep {
  nonceSha256: string;
}

interface WarehouseStep extends HttpStep {
  warehouse: WarehouseSnapshot;
}

interface QueueStep extends HttpStep {
  queue: QueueSnapshot;
}

type Target =
  | "nonce"
  | "verify"
  | "session"
  | "warehouse"
  | "queueJoin"
  | "queueLeave";

class ProductHttpWitness {
  private readonly nonce = new Signal<NonceStep>("SIWE nonce");
  private readonly verify = new Signal<HttpStep>("SIWE verify");
  private readonly session = new Signal<SessionStep>("authenticated session");
  private readonly warehouse = new Signal<WarehouseStep>("warehouse");
  private readonly queueJoin = new Signal<QueueStep>("queue join");
  private readonly queueLeave = new Signal<QueueStep>("queue leave");
  private readonly onResponse = (response: Response) => {
    const target = this.target(response);
    if (target === null) return;
    void this.observe(target, response).catch((error: unknown) => {
      this.fail(target, redactError(error, this.projectId));
    });
  };

  constructor(
    private readonly page: Page,
    private readonly origin: string,
    private readonly expectedAddress: string,
    private readonly projectId: string,
  ) {
    page.on("response", this.onResponse);
  }

  async waitForAuthentication(timeoutMs: number) {
    const [nonce, verify, session, warehouse] = await Promise.all([
      this.nonce.wait(timeoutMs),
      this.verify.wait(timeoutMs),
      this.session.wait(timeoutMs),
      this.warehouse.wait(timeoutMs),
    ]);
    return { nonce, verify, session, warehouse };
  }

  waitForQueueJoin(timeoutMs: number): Promise<QueueStep> {
    return this.queueJoin.wait(timeoutMs);
  }

  waitForQueueLeave(timeoutMs: number): Promise<QueueStep> {
    return this.queueLeave.wait(timeoutMs);
  }

  dispose(): void {
    this.page.off("response", this.onResponse);
  }

  private target(response: Response): Target | null {
    const url = new URL(response.url());
    if (url.origin !== this.origin) return null;
    const method = response.request().method();
    if (url.pathname === "/api/auth/siwe/nonce" && method === "GET")
      return "nonce";
    if (url.pathname === "/api/auth/siwe/verify" && method === "POST")
      return "verify";
    if (url.pathname === "/api/auth/session" && method === "GET")
      return "session";
    if (url.pathname === "/api/warehouse" && method === "GET")
      return "warehouse";
    if (url.pathname === "/api/matchmaking/queue" && method === "POST")
      return "queueJoin";
    if (url.pathname === "/api/matchmaking/queue" && method === "DELETE")
      return "queueLeave";
    return null;
  }

  private async observe(target: Target, response: Response): Promise<void> {
    requireSuccess(response);
    const body = (await response.json()) as unknown;
    const step = baseStep(response);
    switch (target) {
      case "nonce":
        this.nonce.succeed({ ...step, nonceSha256: decodeNonce(body) });
        return;
      case "verify":
        if (body !== true) throw new Error("SIWE verify 未返回 true");
        this.verify.succeed(step);
        return;
      case "session": {
        if (body === null) return;
        const session = decodeSession(body, this.expectedAddress);
        this.session.succeed({ ...step, ...session });
        return;
      }
      case "warehouse":
        this.warehouse.succeed({ ...step, warehouse: decodeWarehouse(body) });
        return;
      case "queueJoin":
        this.queueJoin.succeed({ ...step, queue: decodeQueueSnapshot(body) });
        return;
      case "queueLeave":
        this.queueLeave.succeed({ ...step, queue: decodeQueueSnapshot(body) });
        return;
    }
  }

  private fail(target: Target, error: Error): void {
    switch (target) {
      case "nonce":
        this.nonce.fail(error);
        return;
      case "verify":
        this.verify.fail(error);
        return;
      case "session":
        this.session.fail(error);
        return;
      case "warehouse":
        this.warehouse.fail(error);
        return;
      case "queueJoin":
        this.queueJoin.fail(error);
        return;
      case "queueLeave":
        this.queueLeave.fail(error);
        return;
    }
  }
}

type SignalOutcome<T> = { ok: true; value: T } | { ok: false; error: Error };

class Signal<T> {
  private settled = false;
  private resolve!: (outcome: SignalOutcome<T>) => void;
  private readonly outcome = new Promise<SignalOutcome<T>>((resolve) => {
    this.resolve = resolve;
  });

  constructor(private readonly label: string) {}

  succeed(value: T): void {
    if (this.settled) return;
    this.settled = true;
    this.resolve({ ok: true, value });
  }

  fail(error: Error): void {
    if (this.settled) return;
    this.settled = true;
    this.resolve({ ok: false, error });
  }

  async wait(timeoutMs: number): Promise<T> {
    const outcome = await withTimeout(
      this.outcome,
      timeoutMs,
      `等待 ${this.label} HTTP 证据超时`,
    );
    if (!outcome.ok) throw outcome.error;
    return outcome.value;
  }
}

function decodeNonce(value: unknown): string {
  const source = readRecord(value, "appKitNonce");
  assertOnlyKeys(source, ["nonce", "expiresAt"], "appKitNonce");
  const nonce = readNonEmptyString(source.nonce, "appKitNonce.nonce");
  if (!/^[a-z0-9]{8,}$/iu.test(nonce)) {
    throw new Error("SIWE nonce 格式无效");
  }
  const expiresAt = readNonEmptyString(
    source.expiresAt,
    "appKitNonce.expiresAt",
  );
  if (!Number.isFinite(Date.parse(expiresAt))) {
    throw new Error("SIWE nonce 过期时间无效");
  }
  return hashEvidenceValue(nonce);
}

function decodeSession(
  value: unknown,
  expectedAddress: string,
): { address: string; chainId: 1 } {
  const source = readRecord(value, "appKitSession");
  assertOnlyKeys(source, ["address", "chainId"], "appKitSession");
  const address = readNonEmptyString(source.address, "appKitSession.address");
  if (
    !/^0x[0-9a-f]{40}$/iu.test(address) ||
    address.toLowerCase() !== expectedAddress.toLowerCase() ||
    source.chainId !== ETHEREUM_MAINNET_CHAIN_ID
  ) {
    throw new Error("认证会话与 WalletKit Mainnet EOA 不一致");
  }
  return { address, chainId: ETHEREUM_MAINNET_CHAIN_ID };
}

function baseStep(response: Response): HttpStep {
  return {
    method: response.request().method(),
    observedAt: new Date().toISOString(),
    path: new URL(response.url()).pathname,
    status: response.status(),
  };
}

function requireSuccess(response: Response): void {
  if (response.status() < 200 || response.status() >= 300) {
    throw new Error(
      `产品 HTTP 闭环失败：${response.request().method()} ${new URL(response.url()).pathname} (${response.status()})`,
    );
  }
}

async function waitForLobby(page: Page, timeoutMs: number): Promise<void> {
  await page.locator('[data-product-shell][data-screen="lobby"]').waitFor({
    state: "attached",
    timeout: timeoutMs,
  });
  await expect(page.locator("[data-account-controls]")).toContainText(
    "已认证",
    {
      timeout: timeoutMs,
    },
  );
}

async function assertWarehouseRendered(page: Page): Promise<void> {
  const values = await page
    .locator(".warehouse-section [data-resource] strong")
    .allTextContents();
  if (values.length !== 3 || values.some((value) => !/^\d+$/u.test(value))) {
    throw new Error("登录大厅未显示三类权威仓库数量");
  }
}

function countTransactionRequests(methods: readonly string[]): number {
  return methods.filter((method) => TRANSACTION_METHODS.has(method)).length;
}

async function saveEvidence(
  artifactDir: string,
  testInfo: TestInfo,
  evidence: unknown,
): Promise<void> {
  const body = `${JSON.stringify(evidence, null, 2)}\n`;
  await mkdir(resolve(artifactDir), { recursive: true });
  await writeFile(resolve(artifactDir, "appkit-siwe-gate.json"), body, {
    encoding: "utf8",
    flag: "wx",
  });
  await testInfo.attach("appkit-siwe-evidence", {
    body,
    contentType: "application/json",
  });
}

function sourceEvidence(): {
  gitCommit: string;
  trackedDiffFromHeadSha256: string;
  trackedWorktreeClean: boolean;
} {
  const gitRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();
  if (gitRoot === "") throw new Error("无法解析 Git 仓库根目录");
  try {
    execFileSync(
      "git",
      ["ls-files", "--error-unmatch", "--", ...GATE_SOURCE_PATHS],
      { cwd: gitRoot, stdio: "ignore" },
    );
  } catch {
    throw new Error("AppKit 验收源码必须先纳入 Git 跟踪");
  }
  const gitCommit = execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: gitRoot,
    encoding: "utf8",
  }).trim();
  if (!/^[0-9a-f]{40}$/u.test(gitCommit)) {
    throw new Error("无法解析验收源码 Git commit");
  }
  const trackedDiff = execFileSync(
    "git",
    ["diff", "HEAD", "--binary", "--no-ext-diff"],
    { cwd: gitRoot, encoding: "buffer" },
  );
  return {
    gitCommit,
    trackedDiffFromHeadSha256: createHash("sha256")
      .update(trackedDiff)
      .digest("hex"),
    trackedWorktreeClean: trackedDiff.length === 0,
  };
}

function assertSecretFreeEvidence(value: unknown, projectId: string): void {
  const forbiddenKeys = new Set([
    "cookie",
    "message",
    "mnemonic",
    "pairingUri",
    "privateKey",
    "projectId",
    "sessionTopic",
    "signature",
    "symKey",
  ]);
  const visit = (candidate: unknown): void => {
    if (typeof candidate === "string") {
      if (
        candidate.includes("wc:") ||
        candidate.includes("symKey=") ||
        candidate.includes(projectId)
      ) {
        throw new Error("结构化证据包含禁止的凭证或客户端标识");
      }
      if (/^0x[0-9a-f]{130}$/iu.test(candidate)) {
        throw new Error("结构化证据包含原始签名");
      }
      return;
    }
    if (Array.isArray(candidate)) {
      candidate.forEach(visit);
      return;
    }
    if (candidate === null || typeof candidate !== "object") return;
    for (const [key, child] of Object.entries(candidate)) {
      // HTTP 证据使用 nonce 对象承载哈希和过期时间；只禁止原始 nonce 字符串。
      if (key === "nonce" && typeof child !== "object") {
        throw new Error("结构化证据包含原始 nonce");
      }
      if (forbiddenKeys.has(key)) {
        throw new Error(`结构化证据包含禁止字段 ${key}`);
      }
      visit(child);
    }
  };
  visit(value);
}

function requireProjectId(value: string | undefined): string {
  if (value === undefined || value.trim() === "") {
    throw new Error("AppKit 门禁需要 VITE_REOWN_PROJECT_ID");
  }
  return value.trim();
}

function redactError(error: unknown, projectId: string): Error {
  const raw = error instanceof Error ? error.message : String(error);
  const message = raw
    .replace(/wc:[^\s"'<>]+/giu, "[REDACTED_WC_URI]")
    .replace(/symKey=[^&\s"'<>]+/giu, "symKey=[REDACTED]")
    .replaceAll(projectId, "[REDACTED_REOWN_PROJECT_ID]");
  return new Error(message);
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== null) clearTimeout(timer);
  }
}
