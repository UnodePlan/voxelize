import type { Page } from "@playwright/test";

import {
  canvasEvidenceErrors,
  captureCanvasSample,
  compareCanvasSamples,
  type CanvasDifference,
  type CanvasVisualSample,
} from "./visual-gate-webgl";

export {
  summarizeCanvasSample,
  type CanvasDifference,
  type CanvasVisualSample,
} from "./visual-gate-webgl";

export const LIVE_CANVAS_SELECTOR =
  '[data-world-canvas][data-scene-mode="match"][data-live-world="ready"]';

const CANVAS_POLL_INTERVAL_MS = 250;
const STABILITY_FRAME_ADVANCE = 30;
const REQUIRED_STABLE_COMPARISONS = 2;
const MAX_STABLE_CHANGED_RATIO = 0.003;
const MAX_STABLE_MEAN_CHANNEL_DELTA = 0.75;

export interface CanvasStabilityReport {
  attempts: number;
  frameAfter: number;
  frameBefore: number;
  recentDifferences: CanvasDifference[];
}

export interface StableCanvasSample {
  report: CanvasStabilityReport;
  sample: CanvasVisualSample;
}

export async function waitForCanvasEvidence(
  page: Page,
  deadline: number,
): Promise<CanvasVisualSample> {
  let last = await captureCanvasSample(page, LIVE_CANVAS_SELECTOR);
  let errors = canvasEvidenceErrors(last);
  while (errors.length > 0 && Date.now() < deadline) {
    await page.waitForTimeout(
      Math.min(
        CANVAS_POLL_INTERVAL_MS,
        remainingVisualGateMs(deadline, "WebGL canvas evidence"),
      ),
    );
    last = await captureCanvasSample(page, LIVE_CANVAS_SELECTOR);
    errors = canvasEvidenceErrors(last);
  }
  if (errors.length > 0) {
    throw new Error(
      `WebGL visual evidence failed: ${errors.join(" | ")} (${JSON.stringify(last.metrics)})`,
    );
  }
  return last;
}

export async function waitForRenderFrameAdvance(
  page: Page,
  minimumIncrease: number,
  deadline: number,
): Promise<{ after: number; before: number }> {
  const before = await readRenderFrame(page);
  if (!Number.isFinite(before) || before <= 0) {
    throw new Error(`render frame must be positive, received ${before}`);
  }
  await page.waitForFunction(
    ({ minimum, selector, start }) => {
      const canvas = document.querySelector<HTMLCanvasElement>(selector);
      const frame = Number(canvas?.dataset.renderFrame ?? 0);
      return Number.isFinite(frame) && frame >= start + minimum;
    },
    { minimum: minimumIncrease, selector: LIVE_CANVAS_SELECTOR, start: before },
    { timeout: remainingVisualGateMs(deadline, "render frame advance") },
  );
  return { after: await readRenderFrame(page), before };
}

/** 连续两次跨 30 帧的指纹都稳定，才把最后一帧作为移动比较基线。 */
export async function waitForStableCanvas(
  page: Page,
  deadline: number,
  label: string,
): Promise<StableCanvasSample> {
  let previous = await waitForCanvasEvidence(page, deadline);
  const frameBefore = await readRenderFrame(page);
  const recentDifferences: CanvasDifference[] = [];
  let attempts = 1;
  let stableComparisons = 0;

  while (stableComparisons < REQUIRED_STABLE_COMPARISONS) {
    remainingVisualGateMs(deadline, `${label} stability`);
    await waitForRenderFrameAdvance(page, STABILITY_FRAME_ADVANCE, deadline);
    const current = await waitForCanvasEvidence(page, deadline);
    const difference = compareCanvasSamples(previous, current);
    recentDifferences.push(difference);
    if (recentDifferences.length > REQUIRED_STABLE_COMPARISONS + 1) {
      recentDifferences.shift();
    }
    attempts += 1;
    stableComparisons = isCanvasDifferenceStable(difference)
      ? stableComparisons + 1
      : 0;
    previous = current;
  }

  return {
    report: {
      attempts,
      frameAfter: await readRenderFrame(page),
      frameBefore,
      recentDifferences,
    },
    sample: previous,
  };
}

export function isCanvasDifferenceStable(
  difference: CanvasDifference,
): boolean {
  return (
    difference.changedRatio <= MAX_STABLE_CHANGED_RATIO &&
    difference.meanChannelDelta <= MAX_STABLE_MEAN_CHANNEL_DELTA
  );
}

export async function readRenderFrame(page: Page): Promise<number> {
  return page
    .locator(LIVE_CANVAS_SELECTOR)
    .evaluate((canvas: HTMLCanvasElement) =>
      Number(canvas.dataset.renderFrame ?? 0),
    );
}

export function remainingVisualGateMs(deadline: number, label: string): number {
  const remaining = Math.floor(deadline - Date.now());
  if (remaining <= 0) throw new Error(`${label} exceeded its deadline`);
  return remaining;
}
