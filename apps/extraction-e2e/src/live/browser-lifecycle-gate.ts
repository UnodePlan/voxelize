import { Buffer } from "node:buffer";

import type { Page } from "@playwright/test";

import type { RuntimeDiagnosticsSnapshot } from "./visual-gate-diagnostics";
import { decodeServerFrame } from "./wire";

const WORLD_WORKER_PATTERNS = {
  light: /^light-worker-\d+$/u,
  mesh: /^mesh-worker-\d+$/u,
  urgentMesh: /^mesh-worker-urgent-\d+$/u,
} as const;

export interface WorkerRoleCounts {
  light: number;
  mesh: number;
  other: number;
  protocol: number;
  urgentMesh: number;
}

export interface BrowserLifecycleProfile {
  activeGameSocketCount: number;
  closedGameSocketCount: number;
  gameSocketCount: number;
  liveWorkerCount: number;
  webglCanvasCount: number;
  webglContextCount: number;
  webglContextLostCount: number;
  webglContextRestoredCount: number;
  workerRoles: WorkerRoleCounts;
}

export interface LobbyInputIsolationEvidence {
  outgoingWorldFrames: string[];
  pointerLocked: boolean;
}

export function lifecycleProfile(
  snapshot: RuntimeDiagnosticsSnapshot,
): BrowserLifecycleProfile {
  const workerRoles: WorkerRoleCounts = {
    light: 0,
    mesh: 0,
    other: 0,
    protocol: 0,
    urgentMesh: 0,
  };
  for (const worker of snapshot.workers) {
    if (worker.name === "pvp-protocol") workerRoles.protocol += 1;
    else if (WORLD_WORKER_PATTERNS.mesh.test(worker.name))
      workerRoles.mesh += 1;
    else if (WORLD_WORKER_PATTERNS.urgentMesh.test(worker.name)) {
      workerRoles.urgentMesh += 1;
    } else if (WORLD_WORKER_PATTERNS.light.test(worker.name)) {
      workerRoles.light += 1;
    } else workerRoles.other += 1;
  }
  return {
    activeGameSocketCount: snapshot.activeGameSocketCount,
    closedGameSocketCount: snapshot.closedGameSocketCount,
    gameSocketCount: snapshot.gameSocketCount,
    liveWorkerCount: snapshot.liveWorkerCount,
    webglCanvasCount: snapshot.webglCanvasCount,
    webglContextCount: snapshot.webglContextCount,
    webglContextLostCount: snapshot.webglContextLostCount,
    webglContextRestoredCount: snapshot.webglContextRestoredCount,
    workerRoles,
  };
}

export function baselineLifecycleErrors(
  profile: BrowserLifecycleProfile,
): string[] {
  const errors = consistentProfileErrors(profile);
  if (profile.gameSocketCount !== 1) {
    errors.push("lobby must own exactly one product game socket");
  }
  if (
    profile.activeGameSocketCount !== 1 ||
    profile.closedGameSocketCount !== 0
  ) {
    errors.push("initial lobby game socket must be active and never closed");
  }
  if (profile.workerRoles.protocol !== 1) {
    errors.push("lobby must retain exactly one protocol worker");
  }
  if (worldWorkerCount(profile.workerRoles) !== 0) {
    errors.push("lobby retained World mesh/light workers");
  }
  if (profile.webglCanvasCount !== 1 || profile.webglContextCount !== 1) {
    errors.push("product must retain exactly one canvas WebGL context");
  }
  return errors;
}

export function matchLifecycleErrors(
  profile: BrowserLifecycleProfile,
  baseline: BrowserLifecycleProfile,
  expectedSocketTotal: number,
  expectedRoles?: WorkerRoleCounts,
): string[] {
  const errors = stableRuntimeErrors(profile, baseline);
  if (
    profile.gameSocketCount !== expectedSocketTotal ||
    profile.activeGameSocketCount !== 1 ||
    profile.closedGameSocketCount !== expectedSocketTotal - 1
  ) {
    errors.push("active round game socket lifecycle is inconsistent");
  }
  if (profile.workerRoles.protocol !== baseline.workerRoles.protocol) {
    errors.push("match changed the persistent protocol worker count");
  }
  for (const role of ["light", "mesh", "urgentMesh"] as const) {
    if (profile.workerRoles[role] < 1) {
      errors.push(`match did not create a ${role} World worker`);
    }
  }
  if (
    expectedRoles !== undefined &&
    !sameWorkerRoles(profile.workerRoles, expectedRoles)
  ) {
    errors.push("active World worker counts grew between rounds");
  }
  return errors;
}

export function releasedLifecycleErrors(
  profile: BrowserLifecycleProfile,
  baseline: BrowserLifecycleProfile,
  expectedSocketTotal: number,
): string[] {
  const errors = stableRuntimeErrors(profile, baseline);
  if (
    profile.gameSocketCount !== expectedSocketTotal ||
    profile.activeGameSocketCount !== 0 ||
    profile.closedGameSocketCount !== expectedSocketTotal
  ) {
    errors.push("released round game socket lifecycle is inconsistent");
  }
  if (!sameWorkerRoles(profile.workerRoles, baseline.workerRoles)) {
    errors.push("released World workers did not return to the lobby baseline");
  }
  if (profile.liveWorkerCount !== baseline.liveWorkerCount) {
    errors.push("live Worker count did not return to the lobby baseline");
  }
  return errors;
}

export function inputIsolationErrors(
  evidence: LobbyInputIsolationEvidence,
): string[] {
  const errors: string[] = [];
  if (evidence.pointerLocked) {
    errors.push("released World still accepted pointer lock");
  }
  if (evidence.outgoingWorldFrames.length > 0) {
    errors.push(
      `released World still emitted socket input: ${evidence.outgoingWorldFrames.join(", ")}`,
    );
  }
  return errors;
}

export async function verifyLobbyInputIsolation(
  page: Page,
): Promise<LobbyInputIsolationEvidence> {
  const session = await page.context().newCDPSession(page);
  const outgoingFrames: Uint8Array[] = [];
  try {
    session.on("Network.webSocketFrameSent", ({ response }) => {
      if (response.opcode === 2) {
        outgoingFrames.push(
          Uint8Array.from(Buffer.from(response.payloadData, "base64")),
        );
      }
    });
    await session.send("Network.enable");
    await page.bringToFront();
    const point = await uncoveredCanvasPoint(page);
    await page.mouse.click(point.x, point.y);
    await page.keyboard.down("w");
    await page.waitForTimeout(150);
    await page.keyboard.up("w");
    await page.keyboard.press("q");
    await page.mouse.click(point.x, point.y, { button: "right" });
    await page.waitForTimeout(250);
    return {
      outgoingWorldFrames: outgoingFrames.flatMap(describeWorldBoundFrame),
      pointerLocked: await page.evaluate(
        () => document.pointerLockElement !== null,
      ),
    };
  } finally {
    await page
      .evaluate(() => {
        if (document.pointerLockElement !== null) document.exitPointerLock();
      })
      .catch(() => undefined);
    await session.detach().catch(() => undefined);
  }
}

async function uncoveredCanvasPoint(
  page: Page,
): Promise<{ x: number; y: number }> {
  return page.evaluate(() => {
    const canvas = document.querySelector<HTMLCanvasElement>(
      "[data-world-canvas]",
    );
    if (canvas === null) throw new Error("world canvas is unavailable");
    const bounds = canvas.getBoundingClientRect();
    const inset = 8;
    const candidates = [
      [bounds.right - inset, bounds.bottom - inset],
      [bounds.left + inset, bounds.bottom - inset],
      [bounds.right - inset, bounds.top + bounds.height / 2],
      [bounds.left + inset, bounds.top + bounds.height / 2],
    ];
    const visible = candidates.find(
      ([x, y]) => document.elementFromPoint(x, y) === canvas,
    );
    if (visible === undefined) {
      throw new Error("lobby UI covers every canvas input probe point");
    }
    return { x: visible[0], y: visible[1] };
  });
}

function consistentProfileErrors(profile: BrowserLifecycleProfile): string[] {
  const workerTotal = Object.values(profile.workerRoles).reduce(
    (sum, count) => sum + count,
    0,
  );
  const errors: string[] = [];
  if (workerTotal !== profile.liveWorkerCount) {
    errors.push("Worker role counts do not equal the live Worker count");
  }
  if (profile.webglContextLostCount !== 0) {
    errors.push("a WebGL context was lost");
  }
  if (profile.webglContextRestoredCount !== 0) {
    errors.push("a WebGL context required restoration");
  }
  return errors;
}

function stableRuntimeErrors(
  profile: BrowserLifecycleProfile,
  baseline: BrowserLifecycleProfile,
): string[] {
  const errors = consistentProfileErrors(profile);
  if (
    profile.webglCanvasCount !== baseline.webglCanvasCount ||
    profile.webglContextCount !== baseline.webglContextCount
  ) {
    errors.push("WebGL context count changed between rounds");
  }
  return errors;
}

function worldWorkerCount(roles: WorkerRoleCounts): number {
  return roles.light + roles.mesh + roles.urgentMesh;
}

function sameWorkerRoles(
  left: WorkerRoleCounts,
  right: WorkerRoleCounts,
): boolean {
  return (Object.keys(left) as Array<keyof WorkerRoleCounts>).every(
    (role) => left[role] === right[role],
  );
}

function describeWorldBoundFrame(bytes: Uint8Array): string[] {
  try {
    const frame = decodeServerFrame(bytes);
    if (frame.type === "METHOD" && frame.method?.name.startsWith("pvp:v1:")) {
      return [`METHOD:${frame.method.name}`];
    }
    return ["JOIN", "LEAVE", "LOAD", "PEER", "UNLOAD", "UPDATE"].includes(
      frame.type,
    )
      ? [frame.type]
      : [];
  } catch {
    return [];
  }
}
