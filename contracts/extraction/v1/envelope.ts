import {
  assertOnlyKeys,
  readArray,
  readBoolean,
  readEnum,
  readNonEmptyString,
  readRecord,
  readRequestId,
  readUnsignedInteger,
} from "./decoder-utils";
import type {
  EnvelopeFixture,
  ExtractionManifest,
  ProtocolEnvelope,
  ResultEnvelope,
} from "./types";
import { ERROR_CODES } from "./types";

export function decodeProtocolEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): ProtocolEnvelope {
  const source = readRecord(value, "envelope");
  const type = readNonEmptyString(source.type, "type");

  if (
    readUnsignedInteger(source.protocolVersion, "protocolVersion") !==
    manifest.protocolVersion
  ) {
    throw new Error("protocolVersion: unsupported version");
  }

  if (type === "intent") {
    assertOnlyKeys(
      source,
      ["protocolVersion", "type", "requestId", "sequence", "payload"],
      "intent",
    );
    return {
      protocolVersion: manifest.protocolVersion,
      type,
      requestId: readRequestId(source.requestId),
      sequence: readUnsignedInteger(source.sequence, "sequence"),
      payload: readRecord(source.payload, "payload"),
    };
  }

  if (type === "result") {
    assertOnlyKeys(
      source,
      ["protocolVersion", "type", "requestId", "outcome"],
      "result",
    );
    return {
      protocolVersion: manifest.protocolVersion,
      type,
      requestId: readRequestId(source.requestId),
      outcome: decodeOutcome(source.outcome, manifest),
    };
  }

  throw new Error(`type: unsupported envelope type ${type}`);
}

export function decodeEnvelopeFixture(value: unknown): EnvelopeFixture {
  const source = readRecord(value, "fixture");
  assertOnlyKeys(source, ["fixtureVersion", "cases"], "fixture");

  return {
    fixtureVersion: readUnsignedInteger(
      source.fixtureVersion,
      "fixtureVersion",
    ),
    cases: readArray(source.cases, "cases").map((entry, index) => {
      const fixtureCase = readRecord(entry, `cases[${index}]`);
      assertOnlyKeys(fixtureCase, ["name", "route", "accept", "value"], "case");
      return {
        name: readNonEmptyString(fixtureCase.name, "name"),
        route: readNonEmptyString(fixtureCase.route, "route"),
        accept: readBoolean(fixtureCase.accept, "accept"),
        value: fixtureCase.value,
      };
    }),
  };
}

function decodeOutcome(
  value: unknown,
  manifest: ExtractionManifest,
): ResultEnvelope["outcome"] {
  const source = readRecord(value, "outcome");
  const status = readNonEmptyString(source.status, "outcome.status");

  if (status === "ok") {
    assertOnlyKeys(source, ["status", "data"], "outcome");
    if (!Object.hasOwn(source, "data")) {
      throw new Error("outcome.data: required field is missing");
    }
    return { status, data: source.data };
  }
  if (status === "error") {
    assertOnlyKeys(source, ["status", "error"], "outcome");
    const error = readRecord(source.error, "outcome.error");
    assertOnlyKeys(error, ["code", "retryable"], "outcome.error");
    const code = readEnum(error.code, ERROR_CODES, "outcome.error.code");
    if (!manifest.errorCodes.includes(code)) {
      throw new Error(`outcome.error.code: unknown code ${code}`);
    }
    return {
      status,
      error: {
        code,
        retryable: readBoolean(error.retryable, "outcome.error.retryable"),
      },
    };
  }

  throw new Error(`outcome.status: unsupported status ${status}`);
}
