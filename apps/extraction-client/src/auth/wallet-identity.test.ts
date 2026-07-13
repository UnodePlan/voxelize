import { afterEach, describe, expect, it, vi } from "vitest";

import type { AuthSession } from "../api/auth";

import { WalletIdentityGuard } from "./wallet-identity";

const session: AuthSession = {
  address: "0x1111111111111111111111111111111111111111",
  chainId: 1,
};

describe("WalletIdentityGuard", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("allows AppKit a short restore window after refresh", () => {
    vi.useFakeTimers();
    const invalidate = vi.fn();
    const guard = new WalletIdentityGuard();
    expect(
      guard.validate(
        { configured: true, connected: false, address: null, chainId: null },
        session,
        invalidate,
      ),
    ).toBe("pending");
    expect(
      guard.validate(
        {
          configured: true,
          connected: true,
          address: session.address,
          chainId: 1,
        },
        session,
        invalidate,
      ),
    ).toBe("valid");
    vi.advanceTimersByTime(2_000);
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("invalidates immediately on account or network change", () => {
    const invalidate = vi.fn();
    const guard = new WalletIdentityGuard();
    expect(
      guard.validate(
        {
          configured: true,
          connected: true,
          address: "0x2222222222222222222222222222222222222222",
          chainId: 1,
        },
        session,
        invalidate,
      ),
    ).toBe("invalid");
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it("invalidates immediately when the same address leaves mainnet", () => {
    const invalidate = vi.fn();
    const guard = new WalletIdentityGuard();

    expect(
      guard.validate(
        {
          configured: true,
          connected: true,
          address: session.address,
          chainId: 5,
        },
        session,
        invalidate,
      ),
    ).toBe("invalid");
    expect(invalidate).toHaveBeenCalledTimes(1);
  });

  it("invalidates an established session immediately when the wallet disconnects", () => {
    vi.useFakeTimers();
    const invalidate = vi.fn();
    const guard = new WalletIdentityGuard();

    expect(
      guard.validate(
        {
          configured: true,
          connected: true,
          address: session.address,
          chainId: 1,
        },
        session,
        invalidate,
      ),
    ).toBe("valid");
    expect(
      guard.validate(
        { configured: true, connected: false, address: null, chainId: 1 },
        session,
        invalidate,
      ),
    ).toBe("invalid");
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });
});
