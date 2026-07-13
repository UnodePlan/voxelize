import {
  decodeOptionalMatchResult,
  decodeWarehouse,
} from "../../../extraction-client/src/api/game-decoders";
import type {
  MatchResult,
  WarehouseSnapshot,
} from "../../../extraction-client/src/api/models";

import { requestJson, type LiveHttpTransport } from "./http";

export function fetchLiveWarehouse(
  transport: LiveHttpTransport,
): Promise<WarehouseSnapshot> {
  return requestJson(
    transport,
    "/api/warehouse",
    { method: "GET" },
    decodeWarehouse,
  );
}

export function fetchLiveMatchResult(
  transport: LiveHttpTransport,
  matchId: string,
): Promise<MatchResult | null> {
  if (!/^[0-9a-f]{8}-[0-9a-f-]{27}$/iu.test(matchId)) {
    throw new Error("live match result requires a UUID");
  }
  return requestJson(
    transport,
    `/api/matches/${matchId}/result`,
    { method: "GET" },
    decodeOptionalMatchResult,
  );
}
