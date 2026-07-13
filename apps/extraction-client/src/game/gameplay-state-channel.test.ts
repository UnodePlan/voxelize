import { afterEach, describe, expect, it, vi } from "vitest";

import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import { decodeExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { GameplayStateChannel } from "./gameplay-state-channel";

const manifest = decodeExtractionManifest(manifestJson);

describe("GameplayStateChannel", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("cancels a delayed state sync when the socket closes", async () => {
    vi.useFakeTimers();
    const send = vi.fn();
    const channel = new GameplayStateChannel(manifest, send, vi.fn());

    channel.scheduleSync();
    channel.rejectAll("closed");
    await vi.advanceTimersByTimeAsync(40);

    expect(send).not.toHaveBeenCalled();
  });
});
