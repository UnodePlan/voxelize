import { afterEach, describe, expect, it, vi } from "vitest";

import { Inputs } from "./inputs";

type Listener = EventListenerOrEventListenerObject;

function makeDocumentStub() {
  const listeners = new Map<string, Listener>();
  const addEventListener = vi.fn((type: string, listener: Listener) => {
    listeners.set(type, listener);
  });
  const removeEventListener = vi.fn((type: string, listener: Listener) => {
    if (listeners.get(type) === listener) listeners.delete(type);
  });

  vi.stubGlobal("document", { addEventListener, removeEventListener });

  return { listeners, addEventListener, removeEventListener };
}

function callListener(listener: Listener | undefined, event: unknown): void {
  if (typeof listener === "function") {
    listener(event as Event);
  } else {
    listener?.handleEvent(event as Event);
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Inputs.reset", () => {
  it("removes document keyboard listeners using their original references", () => {
    const { listeners, removeEventListener } = makeDocumentStub();
    const inputs = new Inputs<"game">();
    const callback = vi.fn();

    inputs.setNamespace("game");
    inputs.bind("KeyW", callback, "game");

    callListener(listeners.get("keydown"), { key: "w", code: "KeyW" });
    expect(callback).toHaveBeenCalledOnce();

    inputs.reset();

    expect(listeners.has("keydown")).toBe(false);
    expect(listeners.has("keyup")).toBe(false);
    expect(listeners.has("keypress")).toBe(false);
    expect(removeEventListener).toHaveBeenCalledWith(
      "keydown",
      expect.any(Function),
    );
    expect(removeEventListener).toHaveBeenCalledWith(
      "keyup",
      expect.any(Function),
    );
    expect(removeEventListener).toHaveBeenCalledWith(
      "keypress",
      expect.any(Function),
    );
  });

  it("unbinds every callback and remains idempotent", () => {
    const { listeners, removeEventListener } = makeDocumentStub();
    const inputs = new Inputs<"game">();
    const first = vi.fn();
    const second = vi.fn();

    inputs.setNamespace("game");
    inputs.bind("KeyW", first, "game", { identifier: "first" });
    inputs.bind("KeyW", second, "game", { identifier: "second" });
    const removedKeydownListener = listeners.get("keydown");

    inputs.reset();
    const removeCount = removeEventListener.mock.calls.length;
    inputs.reset();

    callListener(removedKeydownListener, { key: "w", code: "KeyW" });
    expect(first).not.toHaveBeenCalled();
    expect(second).not.toHaveBeenCalled();
    expect(removeEventListener).toHaveBeenCalledTimes(removeCount);
  });
});
