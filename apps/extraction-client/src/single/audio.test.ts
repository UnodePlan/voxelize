import { describe, expect, it } from "vitest";

import { LocalSfx } from "./audio";

describe("LocalSfx", () => {
  it("can be constructed and disposed without throwing in headless env", () => {
    const sfx = new LocalSfx();
    // 未 unlock 时 play 应静默
    expect(() => sfx.play("dig")).not.toThrow();
    expect(() => sfx.dispose()).not.toThrow();
  });
});
