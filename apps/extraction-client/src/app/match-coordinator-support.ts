import type { MessageProtocol } from "@voxelize/protocol";

import type {
  AttackResultData,
  ExtractionManifest,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import type { GameApi } from "../api/game";
import { ApiError } from "../api/http";
import type { QueueSnapshot } from "../api/models";
import type { GameNetworkEvents } from "../game/network";
import type { MovementInput } from "../game/network";

import type { AppAction, AppState } from "./state";

export interface MatchNetwork {
  attack?(): void;
  close(): void;
  connect(): Promise<void>;
  join(worldName: string): void;
  leave(): void;
  mining?(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void;
  movement?(input: MovementInput): void;
  dropSlot?(slot: number, expectedInventoryRevision: number): void;
  /** 局内 PEER 广播昵称 */
  setPeerUsername?(username: string): void;
  requestGameplayState(): Promise<GameplayStateData>;
  resume(worldName: string): void;
  retryResume(): void;
  sendWorldPacket?(message: MessageProtocol): void;
}

export interface MatchCoordinatorOptions {
  dispatch(action: AppAction): void;
  game?: GameApi;
  getState(): AppState;
  networkFactory?: (
    manifest: ExtractionManifest,
    events: GameNetworkEvents,
  ) => MatchNetwork;
  onAuthenticationInvalidated(): void;
  onVoxelMessage?(message: MessageProtocol): void;
  onVoxelReset?(): void;
}

export class MatchGeneration {
  private active = false;
  private generation = 0;
  private queueGeneration = 0;

  constructor(private readonly hasSession: () => boolean) {}

  activate(): void {
    this.generation += 1;
    this.active = true;
    this.queueGeneration += 1;
  }

  close(): void {
    this.active = false;
    this.generation += 1;
    this.queueGeneration += 1;
  }

  token(): number {
    if (!this.active)
      throw new Error("authenticated match session is inactive");
    return this.generation;
  }

  isCurrent(token: number): boolean {
    return this.active && this.generation === token && this.hasSession();
  }

  nextQueue(): number {
    this.queueGeneration += 1;
    return this.queueGeneration;
  }

  queueToken(): number {
    return this.queueGeneration;
  }

  isQueueCurrent(token: number, queueGeneration: number): boolean {
    return this.isCurrent(token) && this.queueGeneration === queueGeneration;
  }
}

interface MatchNetworkEventOptions {
  dispatch(action: AppAction): void;
  getState(): AppState;
  isCurrent(): boolean;
  onAuthenticationInvalidated(): void;
  onConnection?(connection: AppState["connection"]): void;
  onGameplayState?(state: GameplayStateData): void;
  onAttackResult?(result: AttackResultData): void;
  onReconnectExpired(): void;
  startResultPoll(): void;
  stopResultPoll(): void;
  onVoxelMessage?(message: MessageProtocol): void;
  onVoxelReset?(): void;
}

interface MatchLobbyOptions {
  dispatch(action: AppAction): void;
  game: GameApi;
  getState(): AppState;
  handleError(error: unknown): void;
  handleQueue(queue: QueueSnapshot): void;
  isCurrent(): boolean;
  isQueueCurrent(): boolean;
}

interface MatchErrorOptions {
  dispatch(action: AppAction): void;
  error: unknown;
  isCurrent: boolean;
  onAuthenticationInvalidated(): void;
}

export function createMatchNetworkEvents(
  options: MatchNetworkEventOptions,
): GameNetworkEvents {
  return {
    onAuthenticationInvalidated: () => {
      if (options.isCurrent()) options.onAuthenticationInvalidated();
    },
    onConnection: (connection) => {
      if (!options.isCurrent()) return;
      if (options.onConnection === undefined) {
        options.dispatch({ type: "CONNECTION_CHANGED", connection });
      } else {
        options.onConnection(connection);
      }
      if (connection === "reconnecting") options.startResultPoll();
      if (
        connection === "online" &&
        !hasTerminalGameplay(options.getState().gameplay)
      ) {
        options.stopResultPoll();
      }
    },
    onGameplayState: (snapshot) => {
      if (!options.isCurrent()) return;
      options.dispatch({ type: "GAMEPLAY_STATE", state: snapshot });
      options.onGameplayState?.(snapshot);
      if (hasTerminalGameplay(snapshot)) options.startResultPoll();
    },
    onAttackResult: (result) => {
      if (options.isCurrent()) options.onAttackResult?.(result);
    },
    onProtocolError: (message) => {
      if (!options.isCurrent()) return;
      const state = options.getState();
      // 终态驱逐可能与最后一帧输入交错；结果核对负责收敛，不能把预期拒绝显示成新故障。
      if (state.screen === "result" || hasTerminalGameplay(state.gameplay))
        return;
      options.dispatch({ type: "NOTICE", message });
    },
    onReconnectExpired: () => {
      if (options.isCurrent()) options.onReconnectExpired();
    },
    onVoxelMessage: (message) => {
      if (options.isCurrent()) options.onVoxelMessage?.(message);
    },
    onVoxelReset: () => {
      if (options.isCurrent()) options.onVoxelReset?.();
    },
  };
}

export function hasTerminalGameplay(state: GameplayStateData | null): boolean {
  return (
    state !== null &&
    (state.deathResult !== null ||
      state.extraction.data.status === "pending" ||
      state.extraction.data.status === "closed")
  );
}

export async function loadMatchLobby(
  options: MatchLobbyOptions,
): Promise<void> {
  const queueRequest = settled(options.game.getQueue());
  const warehouseRequest = settled(options.game.getWarehouse());
  const resultRequest = settled(options.game.getLatestResult());
  const queue = await queueRequest;
  if (!options.isCurrent()) return;
  if (queue.status === "fulfilled") {
    if (options.isQueueCurrent()) options.handleQueue(queue.value);
  } else {
    options.handleError(queue.reason);
  }
  const [warehouse, result] = await Promise.all([
    warehouseRequest,
    resultRequest,
  ]);
  if (!options.isCurrent()) return;
  if (warehouse.status === "fulfilled") {
    options.dispatch({ type: "WAREHOUSE_READY", warehouse: warehouse.value });
  } else {
    options.handleError(warehouse.reason);
  }
  if (result.status === "fulfilled") {
    options.dispatch({ type: "LATEST_RESULT", result: result.value });
    const currentQueue = options.getState().queue;
    if (currentQueue !== null && options.isQueueCurrent()) {
      options.handleQueue(currentQueue);
    }
  } else {
    options.handleError(result.reason);
  }
}

export function reportMatchError(options: MatchErrorOptions): void {
  if (!options.isCurrent) return;
  if (
    options.error instanceof ApiError &&
    options.error.code === "AUTH_REQUIRED"
  ) {
    options.onAuthenticationInvalidated();
    return;
  }
  options.dispatch({
    type: "NOTICE",
    message: "实时状态刷新失败，正在等待重试",
  });
}

export async function settled<T>(
  promise: Promise<T>,
): Promise<PromiseSettledResult<T>> {
  try {
    return { status: "fulfilled", value: await promise };
  } catch (reason) {
    return { status: "rejected", reason };
  }
}
