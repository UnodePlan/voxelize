import type { MiningStateEnvelope } from "../../../contracts/extraction/v1/typescript";

export interface MiningViewState {
  snapshot: MiningStateEnvelope | null;
}

export const INITIAL_MINING_VIEW_STATE: MiningViewState = {
  snapshot: null,
};

export function reduceMiningState(
  state: MiningViewState,
  incoming: MiningStateEnvelope,
  activeMatchId: string,
): MiningViewState {
  if (incoming.matchId !== activeMatchId) {
    return state;
  }
  const currentRevision = state.snapshot?.revision;
  if (currentRevision !== undefined && incoming.revision <= currentRevision) {
    return state;
  }
  return {
    snapshot: incoming,
  };
}

export function authoritativeMiningProgress(
  state: MiningViewState,
): { elapsedMs: number; requiredMs: number } | null {
  const data = state.snapshot?.data;
  return data?.status === "mining"
    ? { elapsedMs: data.elapsedMs, requiredMs: data.requiredMs }
    : null;
}
