import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";
import { createGameApi } from "../api/game";
import type { QueueSnapshot } from "../api/models";
import { GameNetwork } from "../game/network";

import {
  createMatchNetworkEvents,
  hasTerminalGameplay,
  loadMatchLobby,
  MatchGeneration,
  reportMatchError,
  type MatchCoordinatorOptions,
  type MatchNetwork,
} from "./match-coordinator-support";
import { MatchResultTracker } from "./match-result-tracker";
import { joinableAssignment } from "./match-routing";
import { RepeatingTask } from "./poller";

export class MatchCoordinator {
  private readonly game;
  private readonly networkFactory;
  private network: MatchNetwork | null = null;
  private joinedWorld: string | null = null;
  private readonly queuePoll = new RepeatingTask(1_000);
  private readonly lifecycle;
  private readonly results;

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
    this.networkFactory =
      options.networkFactory ??
      ((manifest, events) => new GameNetwork(manifest, events));
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
    if (assignment === null || !(await this.ensureNetwork(token))) return;
    if (!this.lifecycle.isQueueCurrent(token, queueGeneration)) return;
    if (this.joinedWorld !== assignment.worldName) {
      this.leaveWorld();
      this.options.dispatch({
        type: "MATCH_CONNECTING",
        matchId: assignment.matchId,
        worldName: assignment.worldName,
        reconnecting: false,
      });
      if (queue.status === "preparing") {
        this.network?.join(assignment.worldName);
      } else {
        this.network?.resume(assignment.worldName);
      }
      this.joinedWorld = assignment.worldName;
    }
    if (queue.status === "active" || queue.status === "extractionOpen") {
      try {
        await this.refreshGameplayState(token);
      } catch (error) {
        if (this.lifecycle.isQueueCurrent(token, queueGeneration)) {
          this.network?.retryResume();
        }
        throw error;
      }
    }
  }

  private async refreshGameplayState(token: number): Promise<void> {
    const network = this.network;
    const snapshot = await network?.requestGameplayState();
    if (
      snapshot === undefined ||
      !this.lifecycle.isCurrent(token) ||
      this.network !== network
    ) {
      return;
    }
    this.options.dispatch({ type: "GAMEPLAY_STATE", state: snapshot });
    this.options.dispatch({ type: "MATCH_CONNECTED" });
    if (hasTerminalGameplay(snapshot)) this.results.start(token);
  }

  private async ensureNetwork(token: number): Promise<boolean> {
    const manifest = this.options.getState().manifest;
    if (manifest === null) throw new Error("manifest unavailable");
    if (this.network === null) this.createNetwork(manifest, token);
    const network = this.network;
    await network?.connect();
    if (
      network === undefined ||
      !this.lifecycle.isCurrent(token) ||
      this.network !== network
    ) {
      network?.close();
      return false;
    }
    return true;
  }

  private createNetwork(manifest: ExtractionManifest, token: number): void {
    const current = () => this.lifecycle.isCurrent(token);
    const events = createMatchNetworkEvents({
      dispatch: this.options.dispatch,
      getState: this.options.getState,
      isCurrent: current,
      onAuthenticationInvalidated: this.options.onAuthenticationInvalidated,
      onReconnectExpired: () => this.recoverExpiredMatch(token),
      startResultPoll: () => this.results.start(token),
      stopResultPoll: () => this.results.invalidate(),
    });
    const network = this.networkFactory(manifest, events);
    this.network = network;
  }

  private recoverExpiredMatch(token: number): void {
    this.joinedWorld = null;
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
    if (this.joinedWorld !== null) this.network?.leave();
    this.joinedWorld = null;
  }

  private resetRuntime(): void {
    this.queuePoll.stop();
    this.results.invalidate();
    this.network?.close();
    this.network = null;
    this.joinedWorld = null;
  }

  private beginQueueRequest(): number {
    this.queuePoll.stop();
    this.results.invalidate();
    return this.lifecycle.nextQueue();
  }
}
