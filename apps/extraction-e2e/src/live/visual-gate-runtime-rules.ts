import type { RuntimeDiagnosticsSnapshot } from "./visual-gate-diagnostics";

const REQUIRED_WORKERS = [
  { label: "mesh", pattern: /^mesh-worker-\d+$/u },
  { label: "urgent mesh", pattern: /^mesh-worker-urgent-\d+$/u },
  { label: "light", pattern: /^light-worker-\d+$/u },
] as const;

export function runtimeEvidenceErrors(
  snapshot: RuntimeDiagnosticsSnapshot,
): string[] {
  const errors = [...snapshot.errors];
  errors.push(
    ...snapshot.workerErrors.map(
      ({ kind, message, name }) =>
        `${name || "unnamed worker"} ${kind}: ${message}`,
    ),
    ...snapshot.workerEvaluationErrors.map(
      (error) => `worker evaluation failed: ${error}`,
    ),
  );
  if (snapshot.gameSocketCount < 1) {
    errors.push("game websocket was not observed");
  }
  if (snapshot.activeGameSocketCount !== 1) {
    errors.push("exactly one active game websocket is required");
  }
  if (snapshot.webglContextCount < 1 || snapshot.webglCanvasCount < 1) {
    errors.push("WebGL context was not observed");
  }
  if (snapshot.webglContextLostCount > 0) {
    errors.push("WebGL context was lost");
  }
  for (const required of REQUIRED_WORKERS) {
    const matching = snapshot.workers.filter(({ name }) =>
      required.pattern.test(name),
    );
    if (matching.length === 0) {
      errors.push(`${required.label} worker was not observed`);
    } else if (!matching.some(({ evaluable }) => evaluable)) {
      errors.push(`${required.label} worker was not evaluable`);
    }
  }
  if (snapshot.nonEmptyMeshResponseCount < 1) {
    errors.push("no non-empty mesh geometry response was observed");
  }
  return errors;
}

export function hasDeterministicRuntimeFailure(
  snapshot: RuntimeDiagnosticsSnapshot,
): boolean {
  return (
    snapshot.errors.length > 0 ||
    snapshot.workerErrors.length > 0 ||
    snapshot.workerEvaluationErrors.length > 0
  );
}
