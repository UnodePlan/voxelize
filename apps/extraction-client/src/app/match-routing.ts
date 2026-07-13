import type { MatchResult, QueueSnapshot } from "../api/models";

export interface JoinableAssignment {
  matchId: string;
  worldName: string;
}

export function joinableAssignment(
  queue: QueueSnapshot,
  result: MatchResult | null = null,
): JoinableAssignment | null {
  if (
    queue.matchId === undefined ||
    queue.worldName === undefined ||
    !["preparing", "active", "extractionOpen"].includes(queue.status) ||
    (result?.matchId === queue.matchId &&
      result.status !== "pendingReconciliation")
  ) {
    return null;
  }
  return { matchId: queue.matchId, worldName: queue.worldName };
}
