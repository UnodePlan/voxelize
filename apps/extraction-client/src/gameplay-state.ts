import type {
  GameplayStateData,
  InventoryStateEnvelope,
} from "../../../contracts/extraction/v1/typescript";

export interface GameplayViewState {
  snapshot: GameplayStateData | null;
}

export const INITIAL_GAMEPLAY_VIEW_STATE: GameplayViewState = {
  snapshot: null,
};

export function reduceGameplayState(
  state: GameplayViewState,
  incoming: GameplayStateData,
  activeMatchId: string,
): GameplayViewState {
  if (incoming.matchId !== activeMatchId) {
    return state;
  }
  const current = state.snapshot;
  if (current === null || current.matchId !== incoming.matchId) {
    return { snapshot: incoming };
  }

  // get-state 是跨流原子快照；任一子流回退时整包忽略，防止拼出服务端从未存在的状态。
  const wouldRegress =
    incoming.inventory.revision < current.inventory.revision ||
    incoming.mining.revision < current.mining.revision ||
    incoming.extraction.revision < current.extraction.revision ||
    incoming.health.revision < current.health.revision ||
    incoming.attack.revision < current.attack.revision ||
    (current.deathResult !== null &&
      (incoming.deathResult === null ||
        incoming.deathResult.revision < current.deathResult.revision));
  if (wouldRegress) {
    return state;
  }
  return { snapshot: incoming };
}

export function reduceInventoryState(
  state: GameplayViewState,
  incoming: InventoryStateEnvelope,
  activeMatchId: string,
): GameplayViewState {
  const current = state.snapshot;
  if (
    incoming.matchId !== activeMatchId ||
    current === null ||
    current.matchId !== incoming.matchId ||
    current.health.data.status === "dead" ||
    current.inventory.frozen ||
    incoming.revision <= current.inventory.revision
  ) {
    return state;
  }
  return {
    snapshot: {
      ...current,
      inventory: incoming.data.inventory,
      equipment: incoming.data.equipment,
    },
  };
}
