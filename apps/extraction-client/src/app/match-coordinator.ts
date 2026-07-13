import type { MessageProtocol } from "@voxelize/protocol";

import { createGameApi } from "../api/game";
import type { QueueSnapshot } from "../api/models";
import type { MovementInput } from "../game/network";

import {
  loadMatchLobby,
  MatchGeneration,
  reportMatchError,
  type MatchCoordinatorOptions,
} from "./match-coordinator-support";
import { MatchNetworkRuntime } from "./match-network-runtime";
import { MatchResultTracker } from "./match-result-tracker";
import { joinableAssignment } from "./match-routing";
import { RepeatingTask } from "./poller";

export class MatchCoordinator {
  private readonly game;
  private readonly queuePoll = new RepeatingTask(1_000);
  private readonly lifecycle;
  private readonly results;
  private readonly runtime;

  constructor(private readonly options: MatchCoordinatorOptions) {
    this.game = options.game ?? createGameApi();
    this.lifecycle = new MatchGeneration(
      () => this.options.getState().session !== null,
    );
    this.results = new MatchResultTracker({
      dispatch: this.options.dispatch,
      game: this.game,
      getState: this.options.getState,
      handleError: (error, token) => this.handleError(error, token),
      isCurrent: (token) => this.lifecycle.isCurrent(token),
      leaveWorld: () => this.leaveWorld(),
    });
    this.runtime = new MatchNetworkRuntime({
      dispatch: options.dispatch,
      getState: options.getState,
      isCurrent: (token) => this.lifecycle.isCurrent(token),
      networkFactory: options.networkFactory,
      onAuthenticationInvalidated: options.onAuthenticationInvalidated,
      onReconnectExpired: (token) => this.recoverExpiredMatch(token),
      onVoxelMessage: options.onVoxelMessage,
      onVoxelReset: options.onVoxelReset,
      startResultPoll: (token) => this.results.start(token),
      stopResultPoll: () => this.results.invalidate(),
    });
  }

  activate(): void {
    this.lifecycle.activate();
    this.resetRuntime();
  }

  close(): void {
    this.lifecycle.close();
    this.resetRuntime();
  }

  async loadAuthenticatedSession(): Promise<void> {
    const token = this.lifecycle.token();
    try {
      if (!(await this.ensureNetwork(token))) return;
      await this.loadLobbyFor(token);
    } catch (error) {
      this.handleError(error, token);
    }
  }

  loadLobby(): Promise<void> {
    return this.loadLobbyFor(this.lifecycle.token());
  }

  async joinQueue(): Promise<void> {
    const token = this.lifecycle.token();
    const queueGeneration = this.beginQueueRequest();
    try {
      if (!(await this.ensureNetwork(token))) return;
      const queue = await this.game.joinQueue();
      if (this.lifecycle.isQueueCurrent(token, queueGeneration)) {
        this.handleQueue(queue, token, queueGeneration);
      }
    } catch (error) {
      this.handleError(error, token);
    }
  }

  async leaveQueue(): Promise<void> {
    const token = this.lifecycle.token();
    const queueGeneration = this.beginQueueRequest();
    try {
      const queue = await this.game.leaveQueue();
      if (this.lifecycle.isQueueCurrent(token, queueGeneration)) {
        this.handleQueue(queue, token, queueGeneration);
      }
    } catch (error) {
      this.handleError(error, token);
    }
  }

  async refreshResult(): Promise<void> {
    const token = this.lifecycle.token();
    await this.results.refresh(token);
  }

  attack(): void {
    this.runtime.attack();
  }

  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void {
    this.runtime.mining(action, voxel);
  }

  movement(input: MovementInput): void {
    this.runtime.movement(input);
  }

  dropSlot(slot: number): void {
    const revision = this.options.getState().gameplay?.inventory.revision;
    if (revision !== undefined) this.runtime.dropSlot(slot, revision);
  }

  sendWorldPacket(message: MessageProtocol): void {
    this.runtime.sendWorldPacket(message);
  }

  markWorldReady(): void {
    this.runtime.markWorldReady();
  }

  private async loadLobbyFor(token: number): Promise<void> {
    const queueGeneration = this.beginQueueRequest();
    await loadMatchLobby({
      dispatch: this.options.dispatch,
      game: this.game,
      getState: this.options.getState,
      handleError: (error) => this.handleError(error, token),
      handleQueue: (queue) => this.handleQueue(queue, token, queueGeneration),
      isCurrent: () => this.lifecycle.isCurrent(token),
      isQueueCurrent: () =>
        this.lifecycle.isQueueCurrent(token, queueGeneration),
    });
  }

  private handleQueue(
    queue: QueueSnapshot,
    token: number,
    queueGeneration = this.lifecycle.queueToken(),
  ): void {
    if (!this.lifecycle.isQueueCurrent(token, queueGeneration)) return;
    const previousMatchId = this.options.getState().activeMatchId;
    this.options.dispatch({ type: "QUEUE_CHANGED", queue });
    const state = this.options.getState();
    const assignment = joinableAssignment(queue, state.result);
    if (
      assignment !== null &&
      previousMatchId !== null &&
      previousMatchId !== assignment.matchId
    ) {
      this.results.invalidate();
    }
    const waiting = queue.status === "queued" || queue.status === "preparing";
    if (waiting) {
      this.queuePoll.start(async () => {
        const queueGeneration = this.lifecycle.queueToken();
        try {
          const snapshot = await this.game.getQueue();
          if (this.lifecycle.isQueueCurrent(token, queueGeneration)) {
            this.handleQueue(snapshot, token, queueGeneration);
          }
        } catch (error) {
          this.handleError(error, token);
        }
      });
    } else {
      this.queuePoll.stop();
    }
    if (assignment !== null) {
      void this.enterAssignedMatch(queue, token, queueGeneration).catch(
        (error: unknown) => this.handleError(error, token),
      );
    } else if (queue.status === "settling" && queue.matchId !== undefined) {
      this.leaveWorld();
      this.results.start(token);
    } else {
      this.leaveWorld();
    }
  }

  private async enterAssignedMatch(
    queue: QueueSnapshot,
    token: number,
    queueGeneration: number,
  ): Promise<void> {
    const assignment = joinableAssignment(
      queue,
      this.options.getState().result,
    );
    if (assignment === null || !(await this.runtime.ensure(token))) return;
    if (!this.lifecycle.isQueueCurrent(token, queueGeneration)) return;
    if (!this.runtime.isJoined(assignment.worldName)) {
      this.runtime.enter(
        assignment.matchId,
        assignment.worldName,
        queue.status === "preparing" ? "join" : "resume",
      );
    }
    if (queue.status === "active" || queue.status === "extractionOpen") {
      try {
        await this.runtime.refreshGameplayState(token);
      } catch (error) {
        if (this.lifecycle.isQueueCurrent(token, queueGeneration)) {
          this.runtime.retryResume();
        }
        throw error;
      }
    }
  }

  private async ensureNetwork(token: number): Promise<boolean> {
    return this.runtime.ensure(token);
  }

  private recoverExpiredMatch(token: number): void {
    const queueGeneration = this.beginQueueRequest();
    void this.game.getQueue().then(
      (queue) => {
        if (!this.lifecycle.isQueueCurrent(token, queueGeneration)) return;
        this.handleQueue(queue, token, queueGeneration);
        this.results.start(token);
      },
      (error: unknown) => {
        this.handleError(error, token);
        this.results.start(token);
      },
    );
  }

  private handleError(error: unknown, token: number): void {
    reportMatchError({
      dispatch: this.options.dispatch,
      error,
      isCurrent: this.lifecycle.isCurrent(token),
      onAuthenticationInvalidated: this.options.onAuthenticationInvalidated,
    });
  }

  private leaveWorld(): void {
    this.runtime.leave();
  }

  private resetRuntime(): void {
    this.queuePoll.stop();
    this.results.invalidate();
    this.runtime.reset();
  }

  private beginQueueRequest(): number {
    this.queuePoll.stop();
    this.results.invalidate();
    return this.lifecycle.nextQueue();
  }
}
