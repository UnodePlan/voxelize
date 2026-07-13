import {
  assertOnlyKeys,
  readArray,
  readBoolean,
  readEnum,
  readPositiveInteger,
  readRecord,
  readSignedInteger,
  readUnsignedInteger,
  readUuid,
} from "./decoder-utils";
import type {
  ExtractionStateData,
  ExtractionStateEnvelope,
  ExtractionZoneState,
} from "./extraction-types";
import type { ExtractionManifest } from "./types";

const EXTRACTION_STATUSES = ["hidden", "open", "pending", "closed"] as const;

export function decodeExtractionStateEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): ExtractionStateEnvelope {
  const source = readRecord(value, "extractionState");
  assertOnlyKeys(
    source,
    ["protocolVersion", "type", "matchId", "stream", "revision", "data"],
    "extractionState",
  );
  const protocolVersion = readUnsignedInteger(
    source.protocolVersion,
    "extractionState.protocolVersion",
  );
  if (protocolVersion !== manifest.protocolVersion || source.type !== "state") {
    throw new Error("extractionState: incompatible envelope");
  }
  if (source.stream !== "extraction") {
    throw new Error("extractionState: invalid stream");
  }
  return {
    protocolVersion,
    type: "state",
    matchId: readUuid(source.matchId, "extractionState.matchId"),
    stream: "extraction",
    revision: readUnsignedInteger(source.revision, "extractionState.revision"),
    data: decodeExtractionStateData(source.data),
  };
}

function decodeExtractionStateData(value: unknown): ExtractionStateData {
  const source = readRecord(value, "extractionState.data");
  const status = readEnum(
    source.status,
    EXTRACTION_STATUSES,
    "extractionState.data.status",
  );
  if (status === "hidden" || status === "closed") {
    assertOnlyKeys(source, ["status"], "extractionState.data");
    return { status };
  }
  if (status === "pending") {
    assertOnlyKeys(
      source,
      ["status", "zone", "qualifiedAtUnixSeconds"],
      "extractionState.data",
    );
    return {
      status,
      zone: decodeZone(source.zone),
      qualifiedAtUnixSeconds: readPositiveInteger(
        source.qualifiedAtUnixSeconds,
        "extractionState.data.qualifiedAtUnixSeconds",
      ),
    };
  }
  assertOnlyKeys(
    source,
    [
      "status",
      "zone",
      "inside",
      "elapsedMs",
      "requiredMs",
      "hardDeadlineUnixSeconds",
    ],
    "extractionState.data",
  );
  const elapsedMs = readUnsignedInteger(
    source.elapsedMs,
    "extractionState.data.elapsedMs",
  );
  const requiredMs = readPositiveInteger(
    source.requiredMs,
    "extractionState.data.requiredMs",
  );
  if (elapsedMs > requiredMs) {
    throw new Error("extractionState.data: elapsed exceeds required");
  }
  return {
    status,
    zone: decodeZone(source.zone),
    inside: readBoolean(source.inside, "extractionState.data.inside"),
    elapsedMs,
    requiredMs,
    hardDeadlineUnixSeconds: readPositiveInteger(
      source.hardDeadlineUnixSeconds,
      "extractionState.data.hardDeadlineUnixSeconds",
    ),
  };
}

function decodeZone(value: unknown): ExtractionZoneState {
  const source = readRecord(value, "extractionState.data.zone");
  assertOnlyKeys(
    source,
    ["center", "radiusBlocks", "halfHeightBlocks"],
    "extractionState.data.zone",
  );
  const center = readArray(source.center, "extractionState.data.zone.center");
  if (center.length !== 3) {
    throw new Error(
      "extractionState.data.zone.center: expected three coordinates",
    );
  }
  return {
    center: center.map((coordinate, index) =>
      readSignedInteger(
        coordinate,
        `extractionState.data.zone.center[${index}]`,
      ),
    ) as [number, number, number],
    radiusBlocks: readPositiveInteger(
      source.radiusBlocks,
      "extractionState.data.zone.radiusBlocks",
    ),
    halfHeightBlocks: readPositiveInteger(
      source.halfHeightBlocks,
      "extractionState.data.zone.halfHeightBlocks",
    ),
  };
}
