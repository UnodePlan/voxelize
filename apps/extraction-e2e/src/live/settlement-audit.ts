import {
  assertOnlyKeys,
  readEnum,
  readRecord,
  readUnsignedInteger,
} from "../../../../contracts/extraction/v1/decoder-utils";

import { requestJson, type LiveHttpTransport } from "./http";

const MATCH_STATES = [
  "waiting",
  "preparing",
  "active",
  "extraction_open",
  "settling",
  "finished",
  "aborted",
] as const;
const PARTICIPANT_STATES = [
  "active",
  "disconnected",
  "settlement_pending",
  "dead",
  "extracted",
  "timed_out",
  "aborted",
] as const;

export interface SettlementAuditResourceCounts {
  diamond: number;
  dirt: number;
  gold: number;
}

export interface SettlementAuditSnapshot {
  ledgerCount: number;
  ledgerResources: SettlementAuditResourceCounts;
  matchState: (typeof MATCH_STATES)[number];
  participantState: (typeof PARTICIPANT_STATES)[number];
  settlementCount: number;
  settlementItemCount: number;
  settlementResources: SettlementAuditResourceCounts;
  warehouseResources: SettlementAuditResourceCounts;
  warehouseRowCount: number;
}

export function fetchSettlementAudit(
  transport: LiveHttpTransport,
  matchId: string,
): Promise<SettlementAuditSnapshot | null> {
  if (!/^[0-9a-f]{8}-[0-9a-f-]{27}$/iu.test(matchId)) {
    throw new Error("settlement audit requires a UUID");
  }
  return requestJson(
    transport,
    `/api/e2e/settlements/${matchId}`,
    { method: "GET" },
    decodeSettlementAudit,
  );
}

export function decodeSettlementAudit(
  value: unknown,
): SettlementAuditSnapshot | null {
  if (value === null) return null;
  const source = readRecord(value, "settlementAudit");
  assertOnlyKeys(
    source,
    [
      "matchState",
      "participantState",
      "settlementCount",
      "settlementItemCount",
      "ledgerCount",
      "warehouseRowCount",
      "settlementResources",
      "ledgerResources",
      "warehouseResources",
    ],
    "settlementAudit",
  );
  return {
    matchState: readEnum(
      source.matchState,
      MATCH_STATES,
      "settlementAudit.matchState",
    ),
    participantState: readEnum(
      source.participantState,
      PARTICIPANT_STATES,
      "settlementAudit.participantState",
    ),
    settlementCount: readUnsignedInteger(
      source.settlementCount,
      "settlementAudit.settlementCount",
    ),
    settlementItemCount: readUnsignedInteger(
      source.settlementItemCount,
      "settlementAudit.settlementItemCount",
    ),
    ledgerCount: readUnsignedInteger(
      source.ledgerCount,
      "settlementAudit.ledgerCount",
    ),
    warehouseRowCount: readUnsignedInteger(
      source.warehouseRowCount,
      "settlementAudit.warehouseRowCount",
    ),
    settlementResources: decodeResources(
      source.settlementResources,
      "settlementAudit.settlementResources",
    ),
    ledgerResources: decodeResources(
      source.ledgerResources,
      "settlementAudit.ledgerResources",
    ),
    warehouseResources: decodeResources(
      source.warehouseResources,
      "settlementAudit.warehouseResources",
    ),
  };
}

function decodeResources(
  value: unknown,
  path: string,
): SettlementAuditResourceCounts {
  const source = readRecord(value, path);
  assertOnlyKeys(source, ["dirt", "gold", "diamond"], path);
  return {
    dirt: readUnsignedInteger(source.dirt, `${path}.dirt`),
    gold: readUnsignedInteger(source.gold, `${path}.gold`),
    diamond: readUnsignedInteger(source.diamond, `${path}.diamond`),
  };
}
