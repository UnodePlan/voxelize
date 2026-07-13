import { describe, expect, it } from "vitest";

import { decodeSettlementFaultSnapshot } from "./live/settlement-fault";

const VALID = {
  abortCalls: 0,
  commitCalls: 1,
  commitOutcomeUnknowns: 1,
  markCalls: 1,
  outcomeUnknownArmed: false,
  readsBlocked: true,
  resultReadPassthroughs: 0,
  resultReadUnavailable: 1,
  settlementReadPassthroughs: 0,
  settlementReadUnavailable: 3,
};

describe("settlement fault decoder", () => {
  it("accepts the exact feature-gated control snapshot", () => {
    expect(decodeSettlementFaultSnapshot(VALID)).toEqual(VALID);
  });

  it("rejects unknown fields and invalid counters", () => {
    expect(() =>
      decodeSettlementFaultSnapshot({ ...VALID, databaseUrl: "secret" }),
    ).toThrow(/unknown field databaseUrl/u);
    expect(() =>
      decodeSettlementFaultSnapshot({ ...VALID, commitCalls: -1 }),
    ).toThrow(/unsigned 32-bit integer/u);
    expect(() =>
      decodeSettlementFaultSnapshot({ ...VALID, readsBlocked: "true" }),
    ).toThrow(/boolean/u);
  });
});
