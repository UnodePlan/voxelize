import { describe, expect, it, vi } from "vitest";

import { finishVisualEvidence } from "./visual-gate";

describe("visual gate evidence errors", () => {
  it("preserves the primary gate failure after evidence attachment", async () => {
    const primary = new Error("canvas target crashed");
    const attach = vi.fn(async () => undefined);

    await expect(finishVisualEvidence(primary, attach)).rejects.toBe(primary);
    expect(attach).toHaveBeenCalledOnce();
  });

  it("surfaces an attachment-only failure", async () => {
    const attachment = new Error("screenshot failed");

    await expect(
      finishVisualEvidence(null, async () => {
        throw attachment;
      }),
    ).rejects.toBe(attachment);
  });

  it("keeps both failures with the primary error first", async () => {
    const primary = new Error("locator target crashed");
    const attachment = new Error("screenshot target crashed");

    try {
      await finishVisualEvidence(primary, async () => {
        throw attachment;
      });
      throw new Error("expected visual gate failure");
    } catch (error) {
      expect(error).toBeInstanceOf(AggregateError);
      expect((error as AggregateError).message).toBe(primary.message);
      expect((error as AggregateError).errors).toEqual([primary, attachment]);
    }
  });
});
