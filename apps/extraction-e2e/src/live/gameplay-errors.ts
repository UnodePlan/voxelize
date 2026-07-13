const MAX_U32 = 4_294_967_295;

export class LiveGameplayError extends Error {
  constructor(
    readonly code: string,
    readonly retryable: boolean,
  ) {
    super(`live gameplay request failed: ${code}`);
    this.name = "LiveGameplayError";
  }
}

export class GameplaySocketClosedError extends Error {
  constructor() {
    super("gameplay WebSocket closed");
    this.name = "GameplaySocketClosedError";
  }
}

export function checkedNextU32(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0 || value >= MAX_U32) {
    throw new Error(`${label} exhausted`);
  }
  return value + 1;
}
