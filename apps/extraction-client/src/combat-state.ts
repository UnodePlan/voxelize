import type {
  DeathResultEnvelope,
  HealthStateEnvelope,
} from "../../../contracts/extraction/v1/typescript";

export interface CombatViewState {
  health: HealthStateEnvelope | null;
  deathResult: DeathResultEnvelope | null;
}

export type CombatStateUpdate =
  | { kind: "health"; snapshot: HealthStateEnvelope }
  | { kind: "deathResult"; snapshot: DeathResultEnvelope };

export type HeartFill = "full" | "half" | "empty";

export const INITIAL_COMBAT_VIEW_STATE: CombatViewState = {
  health: null,
  deathResult: null,
};

export function reduceCombatState(
  state: CombatViewState,
  update: CombatStateUpdate,
  activeMatchId: string,
): CombatViewState {
  if (update.snapshot.matchId !== activeMatchId) {
    return state;
  }
  if (update.kind === "health") {
    return reduceHealth(state, update.snapshot);
  }
  return reduceDeathResult(state, update.snapshot);
}

export function projectHearts(state: CombatViewState): HeartFill[] {
  const halfHearts = state.health?.data.currentHalfHearts ?? 0;
  return Array.from({ length: 10 }, (_, index) => {
    const remaining = halfHearts - index * 2;
    return remaining >= 2 ? "full" : remaining === 1 ? "half" : "empty";
  });
}

function reduceHealth(
  state: CombatViewState,
  incoming: HealthStateEnvelope,
): CombatViewState {
  if (state.health !== null && incoming.revision <= state.health.revision) {
    return state;
  }
  if (state.deathResult !== null) {
    if (
      incoming.data.status !== "dead" ||
      incoming.revision !== state.deathResult.revision
    ) {
      return state;
    }
  }
  return { ...state, health: incoming };
}

function reduceDeathResult(
  state: CombatViewState,
  incoming: DeathResultEnvelope,
): CombatViewState {
  if (
    (state.deathResult !== null &&
      incoming.revision <= state.deathResult.revision) ||
    (state.health !== null &&
      (state.health.data.status === "dead"
        ? incoming.revision !== state.health.revision
        : incoming.revision <= state.health.revision))
  ) {
    return state;
  }
  return { ...state, deathResult: incoming };
}
