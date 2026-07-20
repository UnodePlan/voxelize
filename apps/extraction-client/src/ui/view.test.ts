import { describe, expect, it } from "vitest";

import type { MatchResult } from "../api/models";
import { INITIAL_APP_STATE, type AppState } from "../app/state";

import {
  renderResultResources,
  renderWarehouse,
  replaceRenderedHtml,
} from "./view";

// 直接测 render 输出：view 内部用 renderScreen，导出函数已覆盖关键路径。
// 通过 re-render 路径用 export 的 renderWarehouse/result；auth/lobby 用最小状态拼 HTML 断言。
import { renderProductView } from "./view";
import type { ProductShell } from "./shell";

function stubEl(): HTMLElement {
  return {
    innerHTML: "",
    hidden: false,
    dataset: {},
    querySelectorAll: () => [],
    querySelector: () => null,
  } as unknown as HTMLElement;
}

function fakeShell(): ProductShell & {
  accountHtml: () => string;
  contentHtml: () => string;
} {
  const account = stubEl();
  const content = stubEl();
  const hud = stubEl();
  const notice = stubEl();
  const shell = stubEl();
  const canvas = { clientWidth: 1280, clientHeight: 720 } as HTMLCanvasElement;
  return {
    shell,
    canvas,
    content,
    hud,
    account,
    notice,
    accountHtml: () => account.innerHTML,
    contentHtml: () => content.innerHTML,
  };
}

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

  it("unauthenticated screen keeps connect-wallet data-action and access panel", () => {
    const elements = fakeShell();
    const state: AppState = {
      ...INITIAL_APP_STATE,
      screen: "unauthenticated",
      wallet: {
        configured: true,
        connected: false,
        address: null,
        chainId: null,
      },
    };
    renderProductView(state, elements);
    const html = elements.contentHtml();
    expect(html).toContain('class="access-panel side-rail"');
    expect(html).toContain('data-action="connect-wallet"');
    expect(html).toContain("primary-command--hero");
    expect(html).toContain('data-action="retry-bootstrap"');
  });

  it("lobby screen keeps join-queue and refresh-lobby data-action hooks", () => {
    const elements = fakeShell();
    const state: AppState = {
      ...INITIAL_APP_STATE,
      screen: "lobby",
      session: {
        address: "0xabc",
        chainId: 1,
      },
      warehouse: {
        resources: { dirt: 1, gold: 0, diamond: 0 },
        stats: {
          totalResourcesExtracted: 1,
          totalExtractionValue: 1,
          successfulExtractions: 0,
          highestSingleMatchValue: 1,
        },
      },
    };
    renderProductView(state, elements);
    const html = elements.contentHtml();
    expect(html).toContain('class="lobby-panel side-rail"');
    expect(html).toContain('data-action="join-queue"');
    expect(html).toContain('data-action="refresh-lobby"');
    expect(html).toContain("primary-command--hero");
    expect(html).toContain("永久仓库");
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
