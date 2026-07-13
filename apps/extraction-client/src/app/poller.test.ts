import { afterEach, describe, expect, it, vi } from "vitest";

import { RepeatingTask } from "./poller";

afterEach(() => {
  vi.useRealTimers();
});

describe("RepeatingTask", () => {
  it("waits one interval by default and can run immediately", async () => {
    vi.useFakeTimers();
    const delayedTask = vi.fn();
    const delayed = new RepeatingTask(1_000);

    delayed.start(delayedTask);
    expect(delayedTask).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(999);
    expect(delayedTask).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(delayedTask).toHaveBeenCalledTimes(1);

    const immediateTask = vi.fn();
    const immediate = new RepeatingTask(1_000);
    immediate.start(immediateTask, { immediate: true });
    expect(immediateTask).toHaveBeenCalledTimes(1);

    delayed.stop();
    immediate.stop();
  });

  it("never overlaps a slow asynchronous task", async () => {
    vi.useFakeTimers();
    const firstRun = deferred();
    const task = vi
      .fn<[], void | Promise<void>>()
      .mockImplementationOnce(() => firstRun.promise);
    const repeating = new RepeatingTask(1_000);

    repeating.start(task, { immediate: true });
    await vi.advanceTimersByTimeAsync(10_000);
    expect(task).toHaveBeenCalledTimes(1);

    firstRun.resolve();
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(999);
    expect(task).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(task).toHaveBeenCalledTimes(2);

    repeating.stop();
  });

  it("does not schedule again when stopped during an in-flight task", async () => {
    vi.useFakeTimers();
    const inFlight = deferred();
    const task = vi.fn(() => inFlight.promise);
    const repeating = new RepeatingTask(1_000);

    repeating.start(task, { immediate: true });
    repeating.stop();
    inFlight.resolve();
    await vi.advanceTimersByTimeAsync(10_000);

    expect(repeating.running).toBe(false);
    expect(task).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps an old generation from affecting a restarted task", async () => {
    vi.useFakeTimers();
    const oldRun = deferred();
    const newRun = deferred();
    const oldTask = vi.fn(() => oldRun.promise);
    const newTask = vi
      .fn<[], void | Promise<void>>()
      .mockImplementationOnce(() => newRun.promise);
    const repeating = new RepeatingTask(1_000);

    repeating.start(oldTask, { immediate: true });
    repeating.stop();
    repeating.start(newTask, { immediate: true });

    oldRun.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(oldTask).toHaveBeenCalledTimes(1);
    expect(newTask).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);

    newRun.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(1);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(oldTask).toHaveBeenCalledTimes(1);
    expect(newTask).toHaveBeenCalledTimes(2);

    repeating.stop();
  });
});

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve = (): void => undefined;
  const promise = new Promise<void>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
