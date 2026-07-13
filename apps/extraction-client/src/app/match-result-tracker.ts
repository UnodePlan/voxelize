import type { GameApi } from "../api/game";

import { RepeatingTask } from "./poller";
import type { AppAction, AppState } from "./state";

interface MatchResultTrackerOptions {
  dispatch(action: AppAction): void;
  game: GameApi;
  getState(): AppState;
  handleError(error: unknown, token: number): void;
  isCurrent(token: number): boolean;
  leaveWorld(): void;
}

export class MatchResultTracker {
  private generation = 0;
  private readonly poll = new RepeatingTask(1_000);

  constructor(private readonly options: MatchResultTrackerOptions) {}

  invalidate(): void {
    this.generation += 1;
    this.poll.stop();
  }

  async refresh(token: number): Promise<void> {
    const generation = this.generation;
    try {
      await this.refreshFor(token, generation);
    } catch (error) {
      this.options.handleError(error, token);
    }
  }

  start(token: number): void {
    if (!this.options.isCurrent(token)) return;
    const generation = this.generation;
    this.poll.start(
      async () => {
        try {
          await this.refreshFor(token, generation);
        } catch (error) {
          this.options.handleError(error, token);
        }
      },
      { immediate: true },
    );
  }

  private async refreshFor(token: number, generation: number): Promise<void> {
    if (!this.options.isCurrent(token)) return;
    const state = this.options.getState();
    const matchId = state.activeMatchId ?? state.result?.matchId;
    if (matchId === undefined || matchId === null) return;
    const result = await this.options.game.getMatchResult(matchId);
    const current = this.options.getState();
    const currentMatchId = current.activeMatchId ?? current.result?.matchId;
    if (
      !this.options.isCurrent(token) ||
      this.generation !== generation ||
      currentMatchId !== matchId ||
      result === null
    ) {
      return;
    }
    this.options.leaveWorld();
    this.options.dispatch({ type: "MATCH_RESULT", result });
    if (result.status !== "pendingReconciliation") this.poll.stop();
  }
}
