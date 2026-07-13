import type {
  ExtractionManifest,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import type {
  MatchResult,
  QueueSnapshot,
  SessionView,
  WarehouseSnapshot,
} from "../api/models";
import {
  INITIAL_GAMEPLAY_VIEW_STATE,
  reduceGameplayState,
} from "../gameplay-state";

import { joinableAssignment } from "./match-routing";

export type AppScreen =
  | "booting"
  | "unauthenticated"
  | "lobby"
  | "queue"
  | "match"
  | "result";

export interface WalletState {
  configured: boolean;
  connected: boolean;
  address: string | null;
  chainId: number | string | null;
}

export interface AppState {
  screen: AppScreen;
  manifest: ExtractionManifest | null;
  session: SessionView | null;
  wallet: WalletState;
  queue: QueueSnapshot | null;
  warehouse: WarehouseSnapshot | null;
  result: MatchResult | null;
  activeMatchId: string | null;
  worldName: string | null;
  gameplay: GameplayStateData | null;
  connection: "offline" | "connecting" | "online" | "reconnecting";
  busy: string | null;
  notice: string | null;
}

export type AppAction =
  | {
      type: "BOOTSTRAP_READY";
      manifest: ExtractionManifest;
      session: SessionView | null;
    }
  | { type: "BOOTSTRAP_FAILED"; message: string }
  | { type: "WALLET_CONFIGURED"; configured: boolean }
  | {
      type: "WALLET_CHANGED";
      connected: boolean;
      address?: string;
      chainId?: number | string;
    }
  | { type: "SESSION_READY"; session: SessionView }
  | { type: "SESSION_CLEARED"; notice?: string }
  | {
      type: "LOBBY_DATA";
      warehouse: WarehouseSnapshot;
      result: MatchResult | null;
      queue: QueueSnapshot;
    }
  | { type: "WAREHOUSE_READY"; warehouse: WarehouseSnapshot }
  | { type: "LATEST_RESULT"; result: MatchResult | null }
  | { type: "QUEUE_CHANGED"; queue: QueueSnapshot }
  | {
      type: "MATCH_CONNECTING";
      matchId: string;
      worldName: string;
      reconnecting?: boolean;
    }
  | { type: "MATCH_CONNECTED" }
  | { type: "CONNECTION_CHANGED"; connection: AppState["connection"] }
  | { type: "GAMEPLAY_STATE"; state: GameplayStateData }
  | { type: "MATCH_RESULT"; result: MatchResult }
  | { type: "SHOW_LOBBY" }
  | { type: "BUSY"; operation: string | null }
  | { type: "NOTICE"; message: string | null };

export const INITIAL_APP_STATE: AppState = {
  screen: "booting",
  manifest: null,
  session: null,
  wallet: {
    configured: false,
    connected: false,
    address: null,
    chainId: null,
  },
  queue: null,
  warehouse: null,
  result: null,
  activeMatchId: null,
  worldName: null,
  gameplay: null,
  connection: "offline",
  busy: null,
  notice: null,
};

export function reduceAppState(state: AppState, action: AppAction): AppState {
  if (state.session === null && requiresSession(action.type)) return state;
  switch (action.type) {
    case "BOOTSTRAP_READY":
      return {
        ...state,
        manifest: action.manifest,
        session: action.session,
        screen: action.session === null ? "unauthenticated" : "lobby",
        notice: null,
      };
    case "BOOTSTRAP_FAILED":
      return { ...state, screen: "unauthenticated", notice: action.message };
    case "WALLET_CONFIGURED":
      return {
        ...state,
        wallet: { ...state.wallet, configured: action.configured },
      };
    case "WALLET_CHANGED":
      return {
        ...state,
        wallet: {
          ...state.wallet,
          connected: action.connected,
          address: action.address ?? null,
          chainId: action.chainId ?? null,
        },
      };
    case "SESSION_READY":
      return {
        ...state,
        session: action.session,
        screen: "lobby",
        notice: null,
      };
    case "SESSION_CLEARED":
      return clearSessionState(state, action.notice ?? null);
    case "LOBBY_DATA":
      return applyLobbyData(state, action);
    case "WAREHOUSE_READY":
      return { ...state, warehouse: action.warehouse };
    case "LATEST_RESULT":
      if (action.result === null && state.result !== null) return state;
      if (
        action.result !== null &&
        isFinalResult(state.result, action.result)
      ) {
        return state;
      }
      return { ...state, result: action.result };
    case "QUEUE_CHANGED":
      return applyQueue(state, action.queue);
    case "MATCH_CONNECTING":
      return {
        ...state,
        screen: "match",
        activeMatchId: action.matchId,
        worldName: action.worldName,
        gameplay: null,
        connection: action.reconnecting ? "reconnecting" : "connecting",
      };
    case "MATCH_CONNECTED":
      return { ...state, connection: "online" };
    case "CONNECTION_CHANGED":
      return { ...state, connection: action.connection };
    case "GAMEPLAY_STATE":
      return applyGameplayState(state, action.state);
    case "MATCH_RESULT":
      if (isFinalResult(state.result, action.result)) return state;
      return {
        ...state,
        screen: "result",
        result: action.result,
        worldName: null,
        gameplay: null,
        connection: "offline",
      };
    case "SHOW_LOBBY":
      return {
        ...state,
        screen: "lobby",
        activeMatchId: null,
        worldName: null,
        gameplay: null,
      };
    case "BUSY":
      return { ...state, busy: action.operation };
    case "NOTICE":
      return { ...state, notice: action.message };
  }
}

function requiresSession(type: AppAction["type"]): boolean {
  return [
    "LOBBY_DATA",
    "WAREHOUSE_READY",
    "LATEST_RESULT",
    "QUEUE_CHANGED",
    "MATCH_CONNECTING",
    "MATCH_CONNECTED",
    "CONNECTION_CHANGED",
    "GAMEPLAY_STATE",
    "MATCH_RESULT",
    "SHOW_LOBBY",
  ].includes(type);
}

function isFinalResult(
  current: MatchResult | null,
  incoming: MatchResult,
): boolean {
  return (
    current?.matchId === incoming.matchId &&
    current.status !== "pendingReconciliation"
  );
}

function applyLobbyData(
  state: AppState,
  data: Extract<AppAction, { type: "LOBBY_DATA" }>,
): AppState {
  const withData = {
    ...state,
    warehouse: data.warehouse,
    result: data.result,
    queue: data.queue,
  };
  return applyQueue(withData, data.queue);
}

function applyQueue(state: AppState, queue: QueueSnapshot): AppState {
  const assignment = joinableAssignment(queue, state.result);
  if (assignment !== null) {
    return {
      ...state,
      queue,
      screen: "match",
      activeMatchId: assignment.matchId,
      worldName: assignment.worldName,
      connection:
        state.connection === "offline" ? "connecting" : state.connection,
    };
  }
  if (queue.status === "settling") {
    const result =
      state.result?.matchId === queue.matchId ? state.result : null;
    return {
      ...state,
      queue,
      screen: result === null ? "lobby" : "result",
      result,
      activeMatchId: queue.matchId ?? null,
      worldName: null,
      gameplay: null,
      connection: "offline",
    };
  }
  const waiting = queue.status === "queued" || queue.status === "preparing";
  return {
    ...state,
    queue,
    screen: waiting ? "queue" : "lobby",
    activeMatchId: null,
    worldName: null,
    gameplay: null,
    connection: "offline",
  };
}

function applyGameplayState(
  state: AppState,
  incoming: GameplayStateData,
): AppState {
  if (state.activeMatchId === null) {
    return state;
  }
  const reduced = reduceGameplayState(
    { ...INITIAL_GAMEPLAY_VIEW_STATE, snapshot: state.gameplay },
    incoming,
    state.activeMatchId,
  );
  return reduced.snapshot === state.gameplay
    ? state
    : { ...state, gameplay: reduced.snapshot };
}

function clearSessionState(state: AppState, notice: string | null): AppState {
  return {
    ...state,
    screen: "unauthenticated",
    session: null,
    queue: null,
    warehouse: null,
    result: null,
    activeMatchId: null,
    worldName: null,
    gameplay: null,
    connection: "offline",
    busy: null,
    notice,
  };
}
