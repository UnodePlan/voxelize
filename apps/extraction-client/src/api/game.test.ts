import { afterEach, describe, expect, it, vi } from "vitest";

import { createGameApi } from "./game";

describe("Game API timeout budgets", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reserves the world preparation budget only for queue admission", async () => {
    const timeouts: number[] = [];
    vi.spyOn(AbortSignal, "timeout").mockImplementation((milliseconds) => {
      timeouts.push(milliseconds);
      return new AbortController().signal;
    });
    const api = createGameApi({ request: queueRequest });

    await api.getQueue();
    await api.joinQueue();

    expect(timeouts).toEqual([5_000, 65_000]);
  });

  it("honors an injected timeout for both queue paths", async () => {
    const timeouts: number[] = [];
    vi.spyOn(AbortSignal, "timeout").mockImplementation((milliseconds) => {
      timeouts.push(milliseconds);
      return new AbortController().signal;
    });
    const api = createGameApi({ request: queueRequest, timeoutMs: 123 });

    await api.getQueue();
    await api.joinQueue();

    expect(timeouts).toEqual([123, 123]);
  });
});

const queueRequest = (async () =>
  new Response(JSON.stringify({ status: "idle" }), {
    headers: { "Content-Type": "application/json" },
    status: 200,
  })) as typeof fetch;
