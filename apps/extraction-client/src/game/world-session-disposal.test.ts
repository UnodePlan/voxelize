import { describe, expect, it, vi } from "vitest";

import { runWorldSessionCleanup } from "./world-session-disposal";

describe("runWorldSessionCleanup", () => {
  it("continues through every cleanup step after an earlier failure", () => {
    const first = new Error("input cleanup failed");
    const releaseInput = vi.fn(() => {
      throw first;
    });
    const releaseActors = vi.fn();
    const releaseWorldWorkers = vi.fn();

    const error = runWorldSessionCleanup([
      releaseInput,
      releaseActors,
      releaseWorldWorkers,
    ]);

    expect(error).toBe(first);
    expect(releaseInput).toHaveBeenCalledOnce();
    expect(releaseActors).toHaveBeenCalledOnce();
    expect(releaseWorldWorkers).toHaveBeenCalledOnce();
  });

  it("returns null when every cleanup step succeeds", () => {
    expect(runWorldSessionCleanup([vi.fn(), vi.fn()])).toBeNull();
  });
});
