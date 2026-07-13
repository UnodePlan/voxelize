export interface GameplayTime {
  elapse(milliseconds: number): Promise<GameplayTimeAck>;
}

export interface GameplayTimeAck {
  monotonicMs: number | null;
}

export const realGameplayTime: GameplayTime = {
  async elapse(milliseconds) {
    if (!Number.isSafeInteger(milliseconds) || milliseconds < 0) {
      throw new Error("gameplay elapsed time must be non-negative");
    }
    await new Promise((resolve) => setTimeout(resolve, milliseconds));
    return { monotonicMs: null };
  },
};
