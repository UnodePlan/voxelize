const RECONNECT_BASE_MS = 250;
const RECONNECT_MAX_MS = 2_000;
const RECONNECT_WINDOW_MS = 60_000;

export function reconnectDelay(attempt: number): number {
  return Math.min(RECONNECT_BASE_MS * 2 ** attempt, RECONNECT_MAX_MS);
}

export function startReconnectExpiry(
  callback: () => void,
): ReturnType<typeof setTimeout> {
  return setTimeout(callback, RECONNECT_WINDOW_MS);
}
