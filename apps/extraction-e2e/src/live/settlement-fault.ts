import {
  assertOnlyKeys,
  readBoolean,
  readRecord,
  readUnsignedInteger,
} from "../../../../contracts/extraction/v1/decoder-utils";

import { requestJson, type LiveHttpTransport } from "./http";

export type SettlementFaultAction =
  | "armCommitUnknownAndBlockReads"
  | "releaseReads"
  | "reset";

export interface SettlementFaultSnapshot {
  abortCalls: number;
  commitCalls: number;
  commitOutcomeUnknowns: number;
  markCalls: number;
  outcomeUnknownArmed: boolean;
  readsBlocked: boolean;
  resultReadPassthroughs: number;
  resultReadUnavailable: number;
  settlementReadPassthroughs: number;
  settlementReadUnavailable: number;
}

export function applySettlementFault(
  transport: LiveHttpTransport,
  action: SettlementFaultAction,
): Promise<SettlementFaultSnapshot> {
  return requestJson(
    transport,
    "/api/e2e/settlement-fault",
    {
      method: "POST",
      body: { action },
    },
    decodeSettlementFaultSnapshot,
  );
}

export function fetchSettlementFaultSnapshot(
  transport: LiveHttpTransport,
): Promise<SettlementFaultSnapshot> {
  return requestJson(
    transport,
    "/api/e2e/settlement-fault",
    { method: "GET" },
    decodeSettlementFaultSnapshot,
  );
}

export function decodeSettlementFaultSnapshot(
  value: unknown,
): SettlementFaultSnapshot {
  const source = readRecord(value, "settlementFault");
  const keys = [
    "abortCalls",
    "commitCalls",
    "commitOutcomeUnknowns",
    "markCalls",
    "outcomeUnknownArmed",
    "readsBlocked",
    "resultReadPassthroughs",
    "resultReadUnavailable",
    "settlementReadPassthroughs",
    "settlementReadUnavailable",
  ];
  assertOnlyKeys(source, keys, "settlementFault");
  return {
    abortCalls: readUnsignedInteger(
      source.abortCalls,
      "settlementFault.abortCalls",
    ),
    commitCalls: readUnsignedInteger(
      source.commitCalls,
      "settlementFault.commitCalls",
    ),
    commitOutcomeUnknowns: readUnsignedInteger(
      source.commitOutcomeUnknowns,
      "settlementFault.commitOutcomeUnknowns",
    ),
    markCalls: readUnsignedInteger(
      source.markCalls,
      "settlementFault.markCalls",
    ),
    outcomeUnknownArmed: readBoolean(
      source.outcomeUnknownArmed,
      "settlementFault.outcomeUnknownArmed",
    ),
    readsBlocked: readBoolean(
      source.readsBlocked,
      "settlementFault.readsBlocked",
    ),
    resultReadPassthroughs: readUnsignedInteger(
      source.resultReadPassthroughs,
      "settlementFault.resultReadPassthroughs",
    ),
    resultReadUnavailable: readUnsignedInteger(
      source.resultReadUnavailable,
      "settlementFault.resultReadUnavailable",
    ),
    settlementReadPassthroughs: readUnsignedInteger(
      source.settlementReadPassthroughs,
      "settlementFault.settlementReadPassthroughs",
    ),
    settlementReadUnavailable: readUnsignedInteger(
      source.settlementReadUnavailable,
      "settlementFault.settlementReadUnavailable",
    ),
  };
}
