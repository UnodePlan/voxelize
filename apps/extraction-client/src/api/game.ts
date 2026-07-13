import { apiBaseUrl } from "../api";

import {
  decodeOptionalMatchResult,
  decodeQueueSnapshot,
  decodeWarehouse,
} from "./game-decoders";
import { createHttpClient, type HttpClientOptions } from "./http";
import type { MatchResult, QueueSnapshot, WarehouseSnapshot } from "./models";

export interface GameApi {
  getQueue(): Promise<QueueSnapshot>;
  joinQueue(): Promise<QueueSnapshot>;
  leaveQueue(): Promise<QueueSnapshot>;
  getWarehouse(): Promise<WarehouseSnapshot>;
  getLatestResult(): Promise<MatchResult | null>;
  getMatchResult(matchId: string): Promise<MatchResult | null>;
}

export function createGameApi({
  baseUrl = apiBaseUrl(),
  ...options
}: HttpClientOptions = {}): GameApi {
  const http = createHttpClient({ baseUrl, ...options });
  return {
    getQueue: () =>
      http.requestJson(
        "/api/matchmaking/queue",
        { method: "GET" },
        decodeQueueSnapshot,
      ),
    joinQueue: () =>
      http.requestJson(
        "/api/matchmaking/queue",
        { method: "POST" },
        decodeQueueSnapshot,
      ),
    leaveQueue: () =>
      http.requestJson(
        "/api/matchmaking/queue",
        { method: "DELETE" },
        decodeQueueSnapshot,
      ),
    getWarehouse: () =>
      http.requestJson("/api/warehouse", { method: "GET" }, decodeWarehouse),
    getLatestResult: () =>
      http.requestJson(
        "/api/matches/latest-result",
        { method: "GET" },
        decodeOptionalMatchResult,
      ),
    getMatchResult: (matchId) =>
      http.requestJson(
        `/api/matches/${encodeURIComponent(requireMatchId(matchId))}/result`,
        { method: "GET" },
        decodeOptionalMatchResult,
      ),
  };
}

function requireMatchId(value: string): string {
  if (!/^[0-9a-f-]{36}$/iu.test(value)) {
    throw new Error("matchId: expected UUID");
  }
  return value;
}
