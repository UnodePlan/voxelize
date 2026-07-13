export interface ExtractionZoneState {
  center: [number, number, number];
  radiusBlocks: number;
  halfHeightBlocks: number;
}

export type ExtractionStateData =
  | { status: "hidden" }
  | {
      status: "open";
      zone: ExtractionZoneState;
      inside: boolean;
      elapsedMs: number;
      requiredMs: number;
      hardDeadlineUnixSeconds: number;
    }
  | {
      status: "pending";
      zone: ExtractionZoneState;
      qualifiedAtUnixSeconds: number;
    }
  | { status: "closed" };

export interface ExtractionStateEnvelope {
  protocolVersion: number;
  type: "state";
  matchId: string;
  stream: "extraction";
  revision: number;
  data: ExtractionStateData;
}
