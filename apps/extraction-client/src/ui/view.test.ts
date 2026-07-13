import { describe, expect, it } from "vitest";

import type { MatchResult } from "../api/models";
import { INITIAL_APP_STATE } from "../app/state";

import {
  renderResultResources,
  renderWarehouse,
  replaceRenderedHtml,
} from "./view";

describe("stable product rendering", () => {
  it("preserves existing DOM when the rendered content did not change", () => {
    const element = { innerHTML: "" } as HTMLElement;

    expect(replaceRenderedHtml(element, "<button>排队</button>")).toBe(true);
    const original = element.innerHTML;
    expect(replaceRenderedHtml(element, "<button>排队</button>")).toBe(false);
    expect(element.innerHTML).toBe(original);
    expect(replaceRenderedHtml(element, "<button>取消</button>")).toBe(true);
  });

  it("keeps permanent settlement unknown while reconciliation is pending", () => {
    const html = renderResultResources(pendingResult());

    expect(html).toContain("待确认");
    expect(html).not.toContain("<strong>0</strong>");
  });

  it("shows total extracted resources without inventing unloaded balances", () => {
    expect(renderWarehouse(INITIAL_APP_STATE)).toContain(
      "<dt>累计撤离资源</dt><dd>--</dd>",
    );
    const html = renderWarehouse({
      ...INITIAL_APP_STATE,
      warehouse: {
        resources: { dirt: 4, gold: 3, diamond: 2 },
        stats: {
          totalResourcesExtracted: 9,
          totalExtractionValue: 234,
          successfulExtractions: 1,
          highestSingleMatchValue: 234,
        },
      },
    });
    expect(html).toContain("<dt>累计撤离资源</dt><dd>9</dd>");
  });
});

function pendingResult(): MatchResult {
  const zero = { dirt: 0, gold: 0, diamond: 0 };
  return {
    matchId: "22222222-2222-4222-8222-222222222222",
    status: "pendingReconciliation",
    publicPlayerId: "33333333-3333-4333-8333-333333333333",
    terminalCause: null,
    killerPublicPlayerId: null,
    terminalAt: null,
    survivedMs: null,
    stats: { mined: zero, pickedUp: zero, lost: zero },
    settlement: null,
  };
}
