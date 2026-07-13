import { Buffer } from "node:buffer";

import type { CDPSession, Page } from "@playwright/test";

import {
  LIVE_CANVAS_SELECTOR,
  readRenderFrame,
  remainingVisualGateMs,
  waitForRenderFrameAdvance,
  waitForStableCanvas,
  type CanvasDifference,
  type CanvasStabilityReport,
} from "./visual-gate-canvas";
import {
  movementDifferenceErrors,
  movementInputErrors,
  type MovementInputEvidence,
} from "./visual-gate-motion-rules";
import {
  readAuthoritativeDisplacements,
  readOutgoingMovement,
} from "./visual-gate-movement-frames";
import { compareCanvasSamples } from "./visual-gate-webgl";

const FORWARD_HOLD_MS = 800;
const LOOK_SETTLE_MS = 300;
const SERVER_SETTLE_MS = 500;

export interface ForwardMovementReport {
  baselineStability: CanvasStabilityReport;
  frameAfter: number;
  frameBefore: number;
  input: MovementInputEvidence;
  movementDifference: CanvasDifference;
  postMovementConvergence: CanvasStabilityReport;
}

interface BrowserMovementProbe {
  dispose(): void;
  keyEvents: MovementInputEvidence["keyEvents"];
  mouseEvents: MovementInputEvidence["mouseEvents"];
  pointerLockChanges: boolean[];
}

interface MovementProbe {
  incomingBinaryFrames: Uint8Array[];
  outgoingBinaryFrames: Uint8Array[];
  session: CDPSession;
}

/** 真实输入前后都必须稳定，避免把区块继续加载误判成 W 移动。 */
export async function verifyDesktopForwardMovement(
  page: Page,
  timeoutMs: number,
  playerId: string,
): Promise<ForwardMovementReport> {
  if (playerId.length === 0) throw new Error("movement player ID is empty");
  const deadline = Date.now() + timeoutMs;
  const baseline = await waitForStableCanvas(
    page,
    deadline,
    "pre-input canvas",
  );
  const frameBefore = await readRenderFrame(page);
  const probe = await startMovementProbe(page);
  let keyDown = false;
  try {
    // 两个独立 Context 同时存在时，Pointer Lock 只允许活动文档申请控制。
    await page.bringToFront();
    await page.waitForFunction(() => document.hasFocus(), undefined, {
      timeout: Math.min(
        remainingVisualGateMs(deadline, "desktop focus"),
        10_000,
      ),
    });
    await page.locator(LIVE_CANVAS_SELECTOR).click({
      timeout: remainingVisualGateMs(deadline, "live canvas click"),
    });
    await page.waitForFunction(
      (selector) =>
        document.pointerLockElement === document.querySelector(selector),
      LIVE_CANVAS_SELECTOR,
      {
        timeout: Math.min(
          remainingVisualGateMs(deadline, "pointer lock"),
          10_000,
        ),
      },
    );
    // pointerLockElement 会早于产品监听器完成状态切换，先跨两帧再发键盘事件。
    await waitForRenderFrameAdvance(page, 2, deadline);
    await page.keyboard.down("w");
    keyDown = true;
    await waitForDuration(page, FORWARD_HOLD_MS, deadline, "KeyW hold");
    await page.keyboard.up("w");
    keyDown = false;
    await waitForDuration(page, SERVER_SETTLE_MS, deadline, "server settle");
    await applyTrustedLookInput(page, deadline);

    const convergence = await waitForStableCanvas(
      page,
      deadline,
      "post-input canvas",
    );
    const input = await readMovementEvidence(page, probe, playerId);
    assertMovementInput(input);
    const movementDifference = compareCanvasSamples(
      baseline.sample,
      convergence.sample,
    );
    assertMovementDifference(movementDifference, input);
    return {
      baselineStability: baseline.report,
      frameAfter: await readRenderFrame(page),
      frameBefore,
      input,
      movementDifference,
      postMovementConvergence: convergence.report,
    };
  } finally {
    if (keyDown) await page.keyboard.up("w").catch(() => undefined);
    await stopBrowserMovementProbe(page);
    await page
      .evaluate(() => {
        if (document.pointerLockElement !== null) document.exitPointerLock();
      })
      .catch(() => undefined);
    await probe.session.detach().catch(() => undefined);
  }
}

async function startMovementProbe(page: Page): Promise<MovementProbe> {
  const session = await page.context().newCDPSession(page);
  const incomingBinaryFrames: Uint8Array[] = [];
  const outgoingBinaryFrames: Uint8Array[] = [];
  try {
    session.on("Network.webSocketFrameSent", ({ response }) => {
      if (response.opcode === 2) {
        outgoingBinaryFrames.push(
          Uint8Array.from(Buffer.from(response.payloadData, "base64")),
        );
      }
    });
    session.on("Network.webSocketFrameReceived", ({ response }) => {
      if (response.opcode === 2) {
        incomingBinaryFrames.push(
          Uint8Array.from(Buffer.from(response.payloadData, "base64")),
        );
      }
    });
    await session.send("Network.enable");
    await session.send("Browser.grantPermissions", {
      origin: new URL(page.url()).origin,
      permissions: ["pointerLock"],
    });
    await page.evaluate(() => {
      const target = window as Window & {
        __extractionMovementProbe?: BrowserMovementProbe;
      };
      target.__extractionMovementProbe?.dispose();
      const keyEvents: MovementInputEvidence["keyEvents"] = [];
      const mouseEvents: MovementInputEvidence["mouseEvents"] = [];
      const pointerLockChanges: boolean[] = [];
      const recordKey = (event: KeyboardEvent) => {
        if (event.code === "KeyW") {
          keyEvents.push({
            code: event.code,
            isTrusted: event.isTrusted,
            locked: document.pointerLockElement !== null,
            type: event.type as "keydown" | "keyup",
          });
        }
      };
      const recordPointerLock = () => {
        pointerLockChanges.push(document.pointerLockElement !== null);
      };
      const recordMouse = (event: MouseEvent) => {
        if (document.pointerLockElement !== null) {
          mouseEvents.push({
            isTrusted: event.isTrusted,
            locked: true,
            movementX: event.movementX,
            movementY: event.movementY,
          });
        }
      };
      document.addEventListener("keydown", recordKey, true);
      document.addEventListener("keyup", recordKey, true);
      document.addEventListener("mousemove", recordMouse, true);
      document.addEventListener("pointerlockchange", recordPointerLock);
      target.__extractionMovementProbe = {
        dispose: () => {
          document.removeEventListener("keydown", recordKey, true);
          document.removeEventListener("keyup", recordKey, true);
          document.removeEventListener("mousemove", recordMouse, true);
          document.removeEventListener("pointerlockchange", recordPointerLock);
        },
        keyEvents,
        mouseEvents,
        pointerLockChanges,
      };
    });
    return { incomingBinaryFrames, outgoingBinaryFrames, session };
  } catch (error) {
    await session.detach().catch(() => undefined);
    throw error;
  }
}

async function readMovementEvidence(
  page: Page,
  probe: MovementProbe,
  playerId: string,
): Promise<MovementInputEvidence> {
  const browser = await page.evaluate(() => {
    const target = window as Window & {
      __extractionMovementProbe?: BrowserMovementProbe;
    };
    return {
      keyEvents: target.__extractionMovementProbe?.keyEvents ?? [],
      mouseEvents: target.__extractionMovementProbe?.mouseEvents ?? [],
      pointerLockChanges:
        target.__extractionMovementProbe?.pointerLockChanges ?? [],
    };
  });
  const outgoing = probe.outgoingBinaryFrames.flatMap(readOutgoingMovement);
  return {
    ...browser,
    authoritativeDisplacements: readAuthoritativeDisplacements(
      probe.incomingBinaryFrames,
      playerId,
    ),
    outgoingDirections: outgoing.map(({ direction }) => direction),
    outgoingForwardValues: outgoing.map(({ forward }) => forward),
  };
}

async function stopBrowserMovementProbe(page: Page): Promise<void> {
  await page
    .evaluate(() => {
      const target = window as Window & {
        __extractionMovementProbe?: BrowserMovementProbe;
      };
      target.__extractionMovementProbe?.dispose();
      delete target.__extractionMovementProbe;
    })
    .catch(() => undefined);
}

async function applyTrustedLookInput(
  page: Page,
  deadline: number,
): Promise<void> {
  const canvas = page.locator(LIVE_CANVAS_SELECTOR);
  const box = await canvas.boundingBox();
  if (box === null) throw new Error("live canvas has no input bounds");
  const centerX = box.x + box.width / 2;
  const centerY = box.y + box.height / 2;
  const targetY = Math.min(
    box.y + box.height - 2,
    centerY + Math.max(80, box.height * 0.2),
  );
  await page.mouse.move(centerX, targetY, { steps: 4 });
  await waitForDuration(page, LOOK_SETTLE_MS, deadline, "mouse look settle");
}

function assertMovementInput(evidence: MovementInputEvidence): void {
  const errors = movementInputErrors(evidence);
  if (errors.length > 0) {
    throw new Error(
      `desktop movement input failed: ${errors.join(" | ")} (${JSON.stringify(evidence)})`,
    );
  }
}

function assertMovementDifference(
  difference: CanvasDifference,
  evidence: MovementInputEvidence,
): void {
  const errors = movementDifferenceErrors(difference);
  if (errors.length > 0) {
    throw new Error(
      `stable canvas states did not retain W movement: ${errors.join(" | ")} (${JSON.stringify({ difference, evidence })})`,
    );
  }
}

async function waitForDuration(
  page: Page,
  durationMs: number,
  deadline: number,
  label: string,
): Promise<void> {
  const remaining = remainingVisualGateMs(deadline, label);
  if (remaining < durationMs) throw new Error(`${label} exceeded its deadline`);
  await page.waitForTimeout(durationMs);
}
