import { describe, expect, it, vi } from "vitest";

import type { AuthApi } from "../api/auth";

import { LogoutCoordinator } from "./logout-coordinator";

describe("LogoutCoordinator", () => {
  it("revokes a server cookie even before the local session is activated", async () => {
    const logout = vi.fn(async () => undefined);
    const coordinator = new LogoutCoordinator({ logout } as unknown as AuthApi);

    await expect(coordinator.logout()).resolves.toBe(true);
    expect(logout).toHaveBeenCalledTimes(1);
  });

  it("coalesces account, chain and disconnect invalidation into one logout", async () => {
    const logout = vi.fn(async () => undefined);
    const coordinator = new LogoutCoordinator({ logout } as unknown as AuthApi);
    coordinator.activate();

    const results = await Promise.all([
      coordinator.logout(),
      coordinator.logout(),
      coordinator.logout(),
    ]);

    expect(results).toEqual([true, true, true]);
    expect(logout).toHaveBeenCalledTimes(1);
  });

  it("fails closed without retrying a failed logout in the same session", async () => {
    const logout = vi.fn(async () => Promise.reject(new Error("offline")));
    const coordinator = new LogoutCoordinator({ logout } as unknown as AuthApi);
    coordinator.activate();

    await expect(coordinator.logout()).resolves.toBe(false);
    await expect(coordinator.logout()).resolves.toBe(false);
    expect(logout).toHaveBeenCalledTimes(1);
  });
});
