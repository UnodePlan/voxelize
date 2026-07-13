import type {
  ExtractionManifest,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

export interface SessionView {
  address: string;
  chainId: 1;
}

export type QueueStatus =
  | "idle"
  | "queued"
  | "preparing"
  | "active"
  | "extractionOpen"
  | "settling";

export interface QueueSnapshot {
  status: QueueStatus;
  position?: number;
  enqueuedAt?: string;
  matchId?: string;
  worldName?: string;
  removed?: boolean;
}

export interface ResourceCounts {
  dirt: number;
  gold: number;
  diamond: number;
}

export interface WarehouseSnapshot {
  resources: ResourceCounts;
  stats: {
    totalResourcesExtracted: number;
    totalExtractionValue: number;
    successfulExtractions: number;
    highestSingleMatchValue: number;
  };
}

export type MatchResultStatus =
  | "pendingReconciliation"
  | "extracted"
  | "dead"
  | "timedOut"
  | "aborted";

export type TerminalCause = "melee" | "reconnectTimeout" | "hardDeadline";

export interface MatchResult {
  matchId: string;
  status: MatchResultStatus;
  publicPlayerId: string;
  terminalCause: TerminalCause | null;
  killerPublicPlayerId: string | null;
  terminalAt: string | null;
  survivedMs: number | null;
  stats: {
    mined: ResourceCounts;
    pickedUp: ResourceCounts;
    lost: ResourceCounts;
  };
  settlement: {
    settlementId: string;
    resources: ResourceCounts;
    totalValue: number;
    configVersion: string;
    committedAt: string;
  } | null;
}

export interface ProductBootstrap {
  manifest: ExtractionManifest;
  session: SessionView | null;
  warehouse: WarehouseSnapshot | null;
  latestResult: MatchResult | null;
  queue: QueueSnapshot | null;
}

export interface MatchRuntimeSnapshot {
  matchId: string;
  worldName: string;
  gameplay: GameplayStateData | null;
}
