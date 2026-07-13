import { afterEach, describe, expect, it, vi } from "vitest";

import type { AuthApi } from "../api/auth";
import type { WalletState } from "../app/state";

import { waitForAuthenticatedSession } from "./wait-for-session";

const SESSION = {
  address: "0x1111111111111111111111111111111111111111",
  chainId: 1 as const,
};

describe("waitForAuthenticatedSession", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("accepts a session only when the connected mainnet wallet matches", async () => {
    const auth = {
      getSession: vi.fn(async () => SESSION),
    } as unknown as AuthApi;

    await expect(
      waitForAuthenticatedSession(
        auth,
        () => walletState(SESSION.address, 1),
        () => true,
      ),
    ).resolves.toEqual(SESSION);
  });

  it("rejects an old cookie from another wallet before privileged loading", async () => {
    const auth = {
      getSession: vi.fn(async () => SESSION),
    } as unknown as AuthApi;

    await expect(
      waitForAuthenticatedSession(
        auth,
        () => walletState("0x2222222222222222222222222222222222222222", 1),
        () => true,
      ),
    ).rejects.toThrow("does not match");
  });

  it("rejects a session if the wallet disconnected during SIWE", async () => {
    const auth = {
      getSession: vi.fn(async () => SESSION),
    } as unknown as AuthApi;

    await expect(
      waitForAuthenticatedSession(
        auth,
        () => ({
          ...walletState(null, null),
          connected: false,
        }),
        () => true,
      ),
    ).rejects.toThrow("does not match");
  });

  it("allows a slow wallet selection while the AppKit modal remains open", async () => {
    vi.useFakeTimers();
    let attempts = 0;
    const auth = {
      getSession: vi.fn(async () => {
        attempts += 1;
        return attempts < 61 ? null : SESSION;
      }),
    } as unknown as AuthApi;
    const waiting = waitForAuthenticatedSession(
      auth,
      () => walletState(SESSION.address, 1),
      () => true,
      { pollIntervalMs: 1_000, settleGraceMs: 2_000 },
    );

    await vi.advanceTimersByTimeAsync(60_000);
    await expect(waiting).resolves.toEqual(SESSION);
  });
});

function walletState(
  address: string | null,
  chainId: number | null,
): WalletState {
  return { configured: true, connected: true, address, chainId };
}
