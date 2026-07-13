import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

import { expect, test, type TestInfo } from "@playwright/test";

import type { CapacityAdmission } from "../capacity-scenario";
import { settleConcurrentPhase } from "../concurrent-phase";

import { BrowserCapacityActor } from "./browser-actor";
import {
  baselineLifecycleErrors,
  inputIsolationErrors,
  lifecycleProfile,
  matchLifecycleErrors,
  releasedLifecycleErrors,
  verifyLobbyInputIsolation,
  type BrowserLifecycleProfile,
  type WorkerRoleCounts,
} from "./browser-lifecycle-gate";
import { loadLiveE2eConfig } from "./config";
import { ProtocolCapacityActor } from "./protocol-actor";
import { PrimaryAdmissionBarrier } from "./queue";
import {
  releaseLifecycleProfile,
  type ReleaseLifecycleProfile,
} from "./release-lifecycle-profile";
import { pollUntilMatchReleasedWithLobbyConnections } from "./resource-snapshot";
import { ControlledLiveServer } from "./server-process";
import { deterministicLiveWallet } from "./siwe";
import { runBrowserRuntimeCanvasGate } from "./visual-gate";
import { verifyDesktopForwardMovement } from "./visual-gate-motion";

const SERVER_ORIGIN = "http://127.0.0.1:4215";
const CLIENT_ORIGIN = "http://127.0.0.1:5215";
const DEFAULT_DATABASE_URL = "postgres://localhost/voxelize_extraction_e2e";
const MATCH_SIZE = 10;
const PROTOCOL_PLAYERS = MATCH_SIZE - 1;
const ROUNDS = 3;
const HARD_DEADLINE_AFTER_MS = 720_000;
const RESOURCE_RELEASE_TIMEOUT_MS = 60_000;
const WALLET_OFFSET = 9_000;

test.describe.configure({ mode: "serial" });

test("同一浏览器页面连续三局资源清理门禁 @multi-round", async ({
  browser,
}, testInfo) => {
  test.setTimeout(9 * 60_000);
  const config = loadLiveE2eConfig({
    ...process.env,
    EXTRACTION_E2E_CLIENT_URL: CLIENT_ORIGIN,
    EXTRACTION_E2E_PUBLIC_ORIGIN: CLIENT_ORIGIN,
    EXTRACTION_E2E_SERVER_URL: SERVER_ORIGIN,
    EXTRACTION_E2E_TIMEOUT_MS: "180000",
  });
  const server = await ControlledLiveServer.start({
    databaseUrl: process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL,
    publicOrigin: CLIENT_ORIGIN,
    serverOrigin: SERVER_ORIGIN,
  });
  const initialBarrier = new PrimaryAdmissionBarrier(MATCH_SIZE);
  const browserActor = new BrowserCapacityActor(
    "multi-round-browser",
    browser,
    deterministicLiveWallet(WALLET_OFFSET),
    initialBarrier,
    config,
    { label: "multi-round", viewport: { height: 900, width: 1440 } },
  );
  let activeProtocols: ProtocolCapacityActor[] = [];
  const artifactDir = resolve(config.artifactDir);
  await mkdir(artifactDir, { recursive: true });

  try {
    await browserActor.connect();
    const browserContext = browserActor.visualContext();
    const baseline = await waitForLifecycle(
      browserContext,
      baselineLifecycleErrors,
      config.scenarioTimeoutMs,
      "初始大厅",
    );
    const serverBaseline = await pollUntilMatchReleasedWithLobbyConnections(
      browserActor.liveHttpTransport(),
      1,
      RESOURCE_RELEASE_TIMEOUT_MS,
      "初始单浏览器大厅",
    );
    await attachJson(testInfo, "initial-lobby-lifecycle", {
      browser: baseline.profile,
      server: serverBaseline,
    });
    const initialEvidence = {
      browser: baseline.profile,
      server: serverBaseline,
    };
    let expectedMatchRoles: WorkerRoleCounts | undefined;
    let expectedServerRelease: ReleaseLifecycleProfile | undefined;
    const rounds = [];

    for (let round = 1; round <= ROUNDS; round += 1) {
      const barrier = new PrimaryAdmissionBarrier(MATCH_SIZE);
      browserActor.prepareNextRound(barrier);
      activeProtocols = createRoundProtocols(round, barrier, config);
      const evidence = await runRound({
        baseline: baseline.profile,
        browserActor,
        config,
        expectedMatchRoles,
        protocols: activeProtocols,
        round,
        server,
      });
      expectedMatchRoles ??= evidence.matchProfile.workerRoles;
      const serverRelease = releaseLifecycleProfile(
        evidence.server.zeroConnections,
      );
      if (expectedServerRelease === undefined) {
        expectedServerRelease = serverRelease;
      } else {
        expect(serverRelease).toEqual(expectedServerRelease);
      }
      await attachJson(testInfo, `round-${round}-lifecycle`, evidence);
      rounds.push(evidence);
      await testInfo.attach(`round-${round}-lobby`, {
        body: await browserContext.page.screenshot({ fullPage: true }),
        contentType: "image/png",
      });
      await browserContext.page.screenshot({
        fullPage: true,
        path: resolve(artifactDir, `round-${round}-lobby.png`),
      });
      activeProtocols = [];
    }
    await writeFile(
      resolve(artifactDir, "browser-multi-round-gate.json"),
      `${JSON.stringify(
        {
          schema: "extraction.e2e.browser-multi-round.v1",
          createdAt: new Date().toISOString(),
          initial: initialEvidence,
          rounds,
        },
        null,
        2,
      )}\n`,
      "utf8",
    );
  } catch (cause) {
    const message = cause instanceof Error ? cause.message : String(cause);
    throw new Error(
      `${message}\n最近的受控 Engine 日志：\n${server.diagnostics()}`,
      {
        cause,
      },
    );
  } finally {
    await disconnectProtocols(activeProtocols);
    await browserActor.disconnect();
    await server.stop();
  }
});

interface RoundOptions {
  baseline: BrowserLifecycleProfile;
  browserActor: BrowserCapacityActor;
  config: ReturnType<typeof loadLiveE2eConfig>;
  expectedMatchRoles: WorkerRoleCounts | undefined;
  protocols: ProtocolCapacityActor[];
  round: number;
  server: ControlledLiveServer;
}

async function runRound(options: RoundOptions) {
  const { browserActor, config, protocols, round, server } = options;
  try {
    await settleConcurrentPhase(
      `第 ${round} 局协议客户端连接`,
      protocols.map((actor) => actor.connect()),
    );
    const admissions = await settleConcurrentPhase(`第 ${round} 局十人排队`, [
      browserActor.enqueue(),
      ...protocols.map((actor) => actor.enqueue()),
    ]);
    const assignment = requireAssignment(admissions);
    const joins = await settleConcurrentPhase(`第 ${round} 局十人 JOIN`, [
      browserActor.join(assignment.matchId, assignment.worldName),
      ...protocols.map((actor) =>
        actor.join(assignment.matchId, assignment.worldName),
      ),
    ]);
    expect(joins.every((join) => join.status === "joined")).toBe(true);

    const visual = browserActor.visualContext();
    const visualEvidence = await runBrowserRuntimeCanvasGate(
      visual,
      config.scenarioTimeoutMs,
    );
    const movement = await withControlledClock(server, () =>
      verifyDesktopForwardMovement(
        visual.page,
        Math.min(config.scenarioTimeoutMs, 60_000),
        visual.diagnostics.requireGamePlayerId(browserActor.actorId),
      ),
    );
    const active = await waitForLifecycle(
      visual,
      (profile) =>
        matchLifecycleErrors(
          profile,
          options.baseline,
          round,
          options.expectedMatchRoles,
        ),
      config.scenarioTimeoutMs,
      `第 ${round} 局比赛`,
    );

    visual.diagnostics.expectGameSocketClose();
    await server.elapse(HARD_DEADLINE_AFTER_MS);
    await visual.page
      .locator(
        '[data-product-shell][data-screen="result"] .result-panel[data-result="timedOut"]',
      )
      .waitFor({ state: "attached", timeout: config.scenarioTimeoutMs });
    const released = await waitForLifecycle(
      visual,
      (profile) => releasedLifecycleErrors(profile, options.baseline, round),
      config.scenarioTimeoutMs,
      `第 ${round} 局退场`,
    );
    const protocolLobbyConnections =
      await pollUntilMatchReleasedWithLobbyConnections(
        browserActor.liveHttpTransport(),
        PROTOCOL_PLAYERS,
        RESOURCE_RELEASE_TIMEOUT_MS,
        `第 ${round} 局协议客户端大厅`,
      );

    await visual.page.locator('[data-action="back-lobby"]').click();
    await visual.page
      .locator('[data-product-shell][data-screen="lobby"]')
      .waitFor({ state: "attached", timeout: config.scenarioTimeoutMs });
    const inputIsolation = await verifyLobbyInputIsolation(visual.page);
    expect(inputIsolationErrors(inputIsolation)).toEqual([]);
    await expect(visual.page.locator("[data-notice-bar]")).toBeHidden({
      timeout: config.scenarioTimeoutMs,
    });

    await disconnectProtocols(protocols);
    const zeroConnections = await pollUntilMatchReleasedWithLobbyConnections(
      browserActor.liveHttpTransport(),
      0,
      RESOURCE_RELEASE_TIMEOUT_MS,
      `第 ${round} 局连接归零`,
    );

    return {
      inputIsolation,
      matchId: assignment.matchId,
      matchProfile: active.profile,
      movement,
      releasedProfile: released.profile,
      server: { protocolLobbyConnections, zeroConnections },
      visual: visualEvidence,
    };
  } finally {
    await disconnectProtocols(protocols);
  }
}

function createRoundProtocols(
  round: number,
  barrier: PrimaryAdmissionBarrier,
  config: ReturnType<typeof loadLiveE2eConfig>,
): ProtocolCapacityActor[] {
  return Array.from({ length: PROTOCOL_PLAYERS }, (_, index) => {
    const walletIndex = WALLET_OFFSET + round * MATCH_SIZE + index + 1;
    return new ProtocolCapacityActor(
      `round-${round}-protocol-${String(index + 1).padStart(2, "0")}`,
      deterministicLiveWallet(walletIndex),
      true,
      barrier,
      config,
    );
  });
}

function requireAssignment(admissions: readonly CapacityAdmission[]): {
  matchId: string;
  worldName: string;
} {
  expect(admissions).toHaveLength(MATCH_SIZE);
  const first = admissions[0];
  if (first.status !== "accepted") throw new Error("浏览器未获得比赛席位");
  if (
    admissions.some(
      (admission) =>
        admission.status !== "accepted" ||
        admission.matchId !== first.matchId ||
        admission.worldName !== first.worldName,
    )
  ) {
    throw new Error("十个客户端未收敛到同一场比赛");
  }
  return { matchId: first.matchId, worldName: first.worldName };
}

async function waitForLifecycle(
  browser: ReturnType<BrowserCapacityActor["visualContext"]>,
  errorsFor: (profile: BrowserLifecycleProfile) => string[],
  timeoutMs: number,
  label: string,
) {
  const deadline = Date.now() + timeoutMs;
  let lastErrors: string[] = [];
  for (;;) {
    const remainingMs = deadline - Date.now();
    if (remainingMs < 250) {
      throw new Error(
        `${label}资源未稳定：${lastErrors.join(" | ") || "诊断快照未在期限内完成"}`,
      );
    }
    browser.diagnostics.assertNoFailures(browser.actorId);
    const snapshot = await browser.diagnostics.snapshot(
      Math.min(5_000, remainingMs),
    );
    const profile = lifecycleProfile(snapshot);
    lastErrors = errorsFor(profile);
    if (lastErrors.length === 0) return { profile, snapshot };
    await browser.page.waitForTimeout(125);
  }
}

async function disconnectProtocols(
  protocols: readonly ProtocolCapacityActor[],
): Promise<void> {
  const outcomes = await Promise.allSettled(
    protocols.map((actor) => actor.disconnect()),
  );
  const failure = outcomes.find((outcome) => outcome.status === "rejected");
  if (failure?.status === "rejected") throw failure.reason;
}

function attachJson(testInfo: TestInfo, name: string, value: unknown) {
  return testInfo.attach(name, {
    body: JSON.stringify(value, null, 2),
    contentType: "application/json",
  });
}

async function withControlledClock<T>(
  server: ControlledLiveServer,
  operation: () => Promise<T>,
): Promise<T> {
  let active = true;
  const pump = (async () => {
    while (active) await server.elapse(50);
  })();
  try {
    return await Promise.race([operation(), pump.then(() => neverResult<T>())]);
  } finally {
    active = false;
    await pump;
  }
}

function neverResult<T>(): Promise<T> {
  return new Promise(() => undefined);
}
