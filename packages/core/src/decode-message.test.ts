import { afterEach, describe, expect, it, vi } from "vitest";

describe("decode-message entry", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("loads without constructing rendering workers", async () => {
    const Worker = vi.fn();
    vi.stubGlobal("Worker", Worker);

    const entry = await import("./decode-message");

    expect(entry.decodeMessage).toBeTypeOf("function");
    expect(Worker).not.toHaveBeenCalled();
  });
});
