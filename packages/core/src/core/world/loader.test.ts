import { Texture } from "three";
import { afterEach, describe, expect, it, vi } from "vitest";

import { Loader } from "./loader";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("Loader.dispose", () => {
  it("removes the audio unlock listener and releases textures idempotently", () => {
    const addEventListener = vi.fn();
    const removeEventListener = vi.fn();
    vi.stubGlobal("window", { addEventListener, removeEventListener });

    const loader = new Loader();
    const texture = new Texture();
    const disposeTexture = vi.spyOn(texture, "dispose");
    loader.textures.set("terrain", texture);
    loader.images.set("item", {} as HTMLImageElement);

    const unlockListener = addEventListener.mock.calls[0][1];
    loader.dispose();
    loader.dispose();

    expect(addEventListener).toHaveBeenCalledWith(
      "click",
      expect.any(Function),
    );
    expect(removeEventListener).toHaveBeenCalledOnce();
    expect(removeEventListener).toHaveBeenCalledWith("click", unlockListener);
    expect(disposeTexture).toHaveBeenCalledOnce();
    expect(loader.textures.size).toBe(0);
    expect(loader.images.size).toBe(0);
    expect(loader.audioBuffers.size).toBe(0);
  });
});
