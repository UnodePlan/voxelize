import type { MessageProtocol } from "@voxelize/protocol";

import type {
  ExtractionManifest,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import { GameNetwork, type MovementInput } from "../game/network";

import {
  createMatchNetworkEvents,
  hasTerminalGameplay,
  type MatchCoordinatorOptions,
  type MatchNetwork,
} from "./match-coordinator-support";

interface MatchNetworkRuntimeOptions {
  dispatch: MatchCoordinatorOptions["dispatch"];
  getState: MatchCoordinatorOptions["getState"];
  isCurrent(token: number): boolean;
  networkFactory?: MatchCoordinatorOptions["networkFactory"];
  onAuthenticationInvalidated(): void;
  onReconnectExpired(token: number): void;
  onVoxelMessage?: MatchCoordinatorOptions["onVoxelMessage"];
  onVoxelReset?: MatchCoordinatorOptions["onVoxelReset"];
  startResultPoll(token: number): void;
  stopResultPoll(): void;
}

export class MatchNetworkRuntime {
  private network: MatchNetwork | null = null;
  private joinedWorld: string | null = null;
  private gameplayReady = false;
  private worldReady = false;

  constructor(private readonly options: MatchNetworkRuntimeOptions) {}

  isJoined(worldName: string): boolean {
    return this.joinedWorld === worldName;
  }

  async ensure(token: number): Promise<boolean> {
    const manifest = this.options.getState().manifest;
    if (manifest === null) throw new Error("manifest unavailable");
    if (this.network === null) this.createNetwork(manifest, token);
    const network = this.network;
    await network?.connect();
    if (
      network === undefined ||
      !this.options.isCurrent(token) ||
      this.network !== network
    ) {
      network?.close();
      return false;
    }
    return true;
  }

  enter(matchId: string, worldName: string, mode: "join" | "resume"): void {
    this.leave();
    this.resetReadiness();
    this.options.dispatch({
      type: "MATCH_CONNECTING",
      matchId,
      worldName,
      reconnecting: false,
    });
    if (mode === "join") this.network?.join(worldName);
    else this.network?.resume(worldName);
    this.joinedWorld = worldName;
  }

  async refreshGameplayState(token: number): Promise<void> {
    const network = this.network;
    const snapshot = await network?.requestGameplayState();
    if (
      snapshot === undefined ||
      !this.options.isCurrent(token) ||
      this.network !== network
    ) {
      return;
    }
    this.acceptGameplayState(snapshot, token, true);
  }

  retryResume(): void {
    this.network?.retryResume();
  }

  attack(): void {
    this.network?.attack?.();
  }

  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void {
    this.network?.mining?.(action, voxel);
  }

  movement(input: MovementInput): void {
    this.network?.movement?.(input);
  }

  dropSlot(slot: number, expectedInventoryRevision: number): void {
    this.network?.dropSlot?.(slot, expectedInventoryRevision);
  }

  sendWorldPacket(message: MessageProtocol): void {
    this.network?.sendWorldPacket?.(message);
  }

  markWorldReady(): void {
    this.worldReady = true;
    this.markConnectedWhenReady();
  }

  leave(): void {
    if (this.joinedWorld !== null) this.network?.leave();
    this.joinedWorld = null;
    this.resetReadiness();
  }

  reset(): void {
    this.network?.close();
    this.network = null;
    this.joinedWorld = null;
    this.resetReadiness();
    this.options.onVoxelReset?.();
  }

  private createNetwork(manifest: ExtractionManifest, token: number): void {
    const current = () => this.options.isCurrent(token);
    const events = createMatchNetworkEvents({
      dispatch: this.options.dispatch,
      getState: this.options.getState,
      isCurrent: current,
      onAuthenticationInvalidated: this.options.onAuthenticationInvalidated,
      onConnection: (connection) => this.handleConnection(connection),
      onGameplayState: (state) => this.acceptGameplayState(state, token, false),
      onReconnectExpired: () => {
        this.joinedWorld = null;
        this.resetReadiness();
        this.options.onReconnectExpired(token);
      },
      startResultPoll: () => this.options.startResultPoll(token),
      stopResultPoll: this.options.stopResultPoll,
      onVoxelMessage: this.options.onVoxelMessage,
      onVoxelReset: () => {
        this.resetReadiness();
        this.options.onVoxelReset?.();
      },
    });
    this.network = this.options.networkFactory
      ? this.options.networkFactory(manifest, events)
      : new GameNetwork(manifest, events);
  }

  private acceptGameplayState(
    state: GameplayStateData,
    token: number,
    dispatch: boolean,
  ): void {
    if (!this.options.isCurrent(token)) return;
    if (dispatch) this.options.dispatch({ type: "GAMEPLAY_STATE", state });
    this.gameplayReady = true;
    this.markConnectedWhenReady();
    if (hasTerminalGameplay(state)) this.options.startResultPoll(token);
  }

  private resetReadiness(): void {
    this.gameplayReady = false;
    this.worldReady = false;
  }

  private handleConnection(
    connection: ReturnType<MatchCoordinatorOptions["getState"]>["connection"],
  ): void {
    if (connection === "online" && this.joinedWorld !== null) {
      this.markConnectedWhenReady();
      return;
    }
    this.options.dispatch({ type: "CONNECTION_CHANGED", connection });
  }

  private markConnectedWhenReady(): void {
    if (this.gameplayReady && this.worldReady && this.joinedWorld !== null) {
      this.options.dispatch({ type: "MATCH_CONNECTED" });
    }
  }
}
