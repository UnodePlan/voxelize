import type { TestInfo } from "@playwright/test";

import type { BrowserActorVisualContext } from "./browser-actor";
import {
  remainingVisualGateMs,
  summarizeCanvasSample,
  waitForCanvasEvidence,
  waitForRenderFrameAdvance,
} from "./visual-gate-canvas";
import type { RuntimeDiagnosticsSnapshot } from "./visual-gate-diagnostics";
import {
  inspectVisualLayout,
  type VisualLayoutReport,
} from "./visual-gate-layout";
import {
  verifyDesktopForwardMovement,
  type ForwardMovementReport,
} from "./visual-gate-motion";
import {
  hasDeterministicRuntimeFailure,
  runtimeEvidenceErrors,
} from "./visual-gate-runtime-rules";

export interface BrowserGateEvidence {
  canvas: unknown;
  diagnostics: RuntimeDiagnosticsSnapshot | null;
  failure?: string;
  frame: { after: number; before: number } | null;
  layout: VisualLayoutReport | null;
}

export interface VisualReleaseGateOptions {
  desktop: BrowserActorVisualContext;
  mobile: BrowserActorVisualContext;
  testInfo: TestInfo;
  timeoutMs: number;
}

export async function runVisualReleaseGate(
  options: VisualReleaseGateOptions,
): Promise<void> {
  const timeoutMs = Math.min(options.timeoutMs, 60_000);
  await inspectBrowser(options.mobile, options.testInfo, timeoutMs);
  await inspectBrowser(options.desktop, options.testInfo, timeoutMs);

  let movement: ForwardMovementReport | null = null;
  let finalRuntime: RuntimeDiagnosticsSnapshot | null = null;
  let failure: string | undefined;
  let primaryError: unknown = null;
  try {
    movement = await verifyDesktopForwardMovement(
      options.desktop.page,
      timeoutMs,
      options.desktop.diagnostics.requireGamePlayerId(options.desktop.actorId),
    );
    options.desktop.diagnostics.assertNoFailures(options.desktop.actorId);
    finalRuntime = await options.desktop.diagnostics.snapshot();
    assertRuntimeEvidence(options.desktop.actorId, finalRuntime);
  } catch (error) {
    failure = errorMessage(error);
    primaryError = error;
  }
  await finishVisualEvidence(primaryError, async () => {
    await attachJson(options.testInfo, "desktop-forward-movement", {
      failure,
      finalRuntime,
      movement,
    });
    await attachScreenshot(
      options.testInfo,
      options.desktop,
      "desktop-after-forward-movement",
    );
  });
}

export async function runBrowserRuntimeCanvasGate(
  browser: BrowserActorVisualContext,
  timeoutMs: number,
): Promise<BrowserGateEvidence> {
  const evidence = emptyBrowserEvidence();
  await collectBrowserEvidence(browser, evidence, Math.min(timeoutMs, 60_000));
  return evidence;
}

async function inspectBrowser(
  browser: BrowserActorVisualContext,
  testInfo: TestInfo,
  timeoutMs: number,
): Promise<void> {
  const evidence = emptyBrowserEvidence();
  let primaryError: unknown = null;
  try {
    await collectBrowserEvidence(browser, evidence, timeoutMs);
  } catch (error) {
    evidence.failure = errorMessage(error);
    primaryError = error;
  }
  await finishVisualEvidence(primaryError, async () => {
    await attachJson(testInfo, `${browser.label}-visual-evidence`, evidence);
    await attachScreenshot(testInfo, browser, `${browser.label}-match`);
  });
}

export async function finishVisualEvidence(
  primaryError: unknown,
  attachEvidence: () => Promise<void>,
): Promise<void> {
  let attachmentError: unknown = null;
  try {
    await attachEvidence();
  } catch (error) {
    attachmentError = error;
  }
  if (primaryError !== null && attachmentError !== null) {
    throw new AggregateError(
      [primaryError, attachmentError],
      errorMessage(primaryError),
    );
  }
  if (primaryError !== null) throw primaryError;
  if (attachmentError !== null) throw attachmentError;
}

function emptyBrowserEvidence(): BrowserGateEvidence {
  return {
    canvas: null,
    diagnostics: null,
    frame: null,
    layout: null,
  };
}

async function collectBrowserEvidence(
  browser: BrowserActorVisualContext,
  evidence: BrowserGateEvidence,
  timeoutMs: number,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  await browser.page
    .locator(
      '[data-world-canvas][data-scene-mode="match"][data-live-world="ready"]',
    )
    .waitFor({
      state: "visible",
      timeout: remainingVisualGateMs(deadline, `${browser.actorId} canvas`),
    });
  await browser.page.locator(".player-hud:not(.player-hud-loading)").waitFor({
    state: "visible",
    timeout: remainingVisualGateMs(deadline, `${browser.actorId} HUD`),
  });
  evidence.diagnostics = await waitForRuntimeEvidence(browser, deadline);
  evidence.layout = await inspectVisualLayout(browser.page, browser.viewport);
  if (evidence.layout.errors.length > 0) {
    throw new Error(
      `${browser.actorId} layout failed: ${evidence.layout.errors.join(" | ")}`,
    );
  }
  evidence.frame = await waitForRenderFrameAdvance(browser.page, 30, deadline);
  const sample = await waitForCanvasEvidence(browser.page, deadline);
  evidence.canvas = summarizeCanvasSample(sample);
  browser.diagnostics.assertNoFailures(browser.actorId);
  evidence.diagnostics = await snapshotWithinDeadline(browser, deadline);
  assertRuntimeEvidence(browser.actorId, evidence.diagnostics);
}

async function waitForRuntimeEvidence(
  browser: BrowserActorVisualContext,
  deadline: number,
): Promise<RuntimeDiagnosticsSnapshot> {
  let snapshot = await snapshotWithinDeadline(browser, deadline);
  let errors = runtimeEvidenceErrors(snapshot);
  while (
    errors.length > 0 &&
    !hasDeterministicRuntimeFailure(snapshot) &&
    Date.now() < deadline
  ) {
    await browser.page.waitForTimeout(
      Math.min(
        250,
        remainingVisualGateMs(deadline, `${browser.actorId} runtime evidence`),
      ),
    );
    snapshot = await snapshotWithinDeadline(browser, deadline);
    errors = runtimeEvidenceErrors(snapshot);
  }
  browser.diagnostics.assertNoFailures(browser.actorId);
  assertRuntimeEvidence(browser.actorId, snapshot);
  return snapshot;
}

function snapshotWithinDeadline(
  browser: BrowserActorVisualContext,
  deadline: number,
): Promise<RuntimeDiagnosticsSnapshot> {
  return browser.diagnostics.snapshot(
    Math.min(
      5_000,
      remainingVisualGateMs(deadline, `${browser.actorId} diagnostics`),
    ),
  );
}

function assertRuntimeEvidence(
  actorId: string,
  snapshot: RuntimeDiagnosticsSnapshot,
): void {
  const errors = runtimeEvidenceErrors(snapshot);
  if (errors.length > 0) {
    throw new Error(`${actorId} runtime failed: ${errors.join(" | ")}`);
  }
}

async function attachJson(
  testInfo: TestInfo,
  name: string,
  value: unknown,
): Promise<void> {
  await testInfo.attach(name, {
    body: JSON.stringify(value, null, 2),
    contentType: "application/json",
  });
}

async function attachScreenshot(
  testInfo: TestInfo,
  browser: BrowserActorVisualContext,
  name: string,
): Promise<void> {
  await testInfo.attach(name, {
    body: await browser.page.screenshot({ fullPage: true }),
    contentType: "image/png",
  });
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
