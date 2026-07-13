import { describe, expect, it } from "vitest";

import { decodeSettlementAudit } from "./live/settlement-audit";

const VALID = {
  matchState: "aborted",
  participantState: "extracted",
  settlementCount: 1,
  settlementItemCount: 1,
  ledgerCount: 1,
  warehouseRowCount: 1,
  settlementResources: { dirt: 1, gold: 0, diamond: 0 },
  ledgerResources: { dirt: 1, gold: 0, diamond: 0 },
  warehouseResources: { dirt: 1, gold: 0, diamond: 0 },
};

describe("settlement audit decoder", () => {
  it("accepts the exact authenticated E2E projection", () => {
    expect(decodeSettlementAudit(VALID)).toEqual(VALID);
    expect(decodeSettlementAudit(null)).toBeNull();
  });

  it("rejects unknown states, fields, and negative counts", () => {
    expect(() =>
      decodeSettlementAudit({ ...VALID, participantState: "active_again" }),
    ).toThrow(/unsupported value/u);
    expect(() => decodeSettlementAudit({ ...VALID, secret: "x" })).toThrow(
      /unknown field secret/u,
    );
    expect(() => decodeSettlementAudit({ ...VALID, ledgerCount: -1 })).toThrow(
      /unsigned 32-bit integer/u,
    );
  });
});
