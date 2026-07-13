import { afterEach, describe, expect, it, vi } from "vitest";

import type { AuthApi } from "../api/auth";

import { createReownWallet } from "./appkit";
import type { LogoutCoordinator } from "./logout-coordinator";

const appKitMocks = vi.hoisted(() => {
  let siweConfig: Record<string, (...args: never[]) => unknown> | null = null;
  const requestProvider = (method: string, params?: unknown) =>
    (
      globalThis as typeof globalThis & {
        ethereum: {
          request(input: { method: string; params?: unknown }): unknown;
        };
      }
    ).ethereum.request({ method, params });
  const appKit = {
    disconnect: vi.fn(() => requestProvider("wallet_revokePermissions")),
    getAccount: vi.fn(() => ({
      address: "0x1111111111111111111111111111111111111111",
      isConnected: true,
    })),
    getChainId: vi.fn(() => 1),
    isOpen: vi.fn(() => true),
    open: vi.fn(() => requestProvider("eth_requestAccounts")),
    subscribeAccount: vi.fn(),
    subscribeNetwork: vi.fn(),
    switchNetwork: vi.fn(() =>
      requestProvider("wallet_switchEthereumChain", [{ chainId: "0x1" }]),
    ),
  };
  return {
    appKit,
    createAppKit: vi.fn(() => appKit),
    createSIWEConfig: vi.fn(
      (config: Record<string, (...args: never[]) => unknown>) => {
        siweConfig = config;
        return config;
      },
    ),
    formatMessage: vi.fn(() => "mainnet SIWE message"),
    getSiweConfig: () => siweConfig,
  };
});

vi.mock("@reown/appkit", () => ({
  createAppKit: appKitMocks.createAppKit,
}));
vi.mock("@reown/appkit-adapter-ethers", () => ({
  EthersAdapter: class EthersAdapter {},
}));
vi.mock("@reown/appkit/networks", () => ({
  mainnet: { chainId: 1, id: 1, name: "Ethereum" },
}));
vi.mock("@reown/appkit-siwe", () => ({
  createSIWEConfig: appKitMocks.createSIWEConfig,
  formatMessage: appKitMocks.formatMessage,
}));

describe("Reown AppKit transaction boundary", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("uses identity and network methods without ever requesting a transaction", async () => {
    const providerRequest = vi.fn(async ({ method }: { method: string }) => {
      if (TRANSACTION_METHODS.has(method)) {
        throw new Error(`transaction method is forbidden: ${method}`);
      }
      return method === "eth_requestAccounts"
        ? ["0x1111111111111111111111111111111111111111"]
        : null;
    });
    vi.stubGlobal("ethereum", { request: providerRequest });
    vi.stubGlobal("window", {
      location: {
        host: "game.example.test",
        origin: "https://game.example.test",
      },
    });
    const auth = {
      getNonce: vi.fn(async () => ({
        nonce: "a1b2c3d4e5f6g7h8",
        expiresAt: "2027-01-15T08:00:00Z",
      })),
      getSession: vi.fn(async () => ({
        address: "0x1111111111111111111111111111111111111111",
        chainId: 1,
      })),
      verifyMessage: vi.fn(async () => true),
    } as unknown as AuthApi;
    const logout = {
      logout: vi.fn(async () => true),
    } as unknown as LogoutCoordinator;

    const wallet = await createReownWallet({
      auth,
      logout,
      projectId: "test-project-id",
    });
    expect(appKitMocks.createAppKit).toHaveBeenCalledWith(
      expect.objectContaining({
        features: {
          analytics: false,
          email: false,
          onramp: false,
          receive: false,
          send: false,
          socials: [],
          swaps: false,
        },
      }),
    );
    const siwe = appKitMocks.getSiweConfig();
    expect(siwe).not.toBeNull();
    await expect(siwe?.getNonce?.()).resolves.toBe("a1b2c3d4e5f6g7h8");
    await expect(siwe?.getSession?.()).resolves.toMatchObject({ chainId: 1 });
    await expect(
      siwe?.verifyMessage?.({ message: "message", signature: "0x01" } as never),
    ).resolves.toBe(true);
    await expect(siwe?.signOut?.()).resolves.toBe(true);
    await wallet.open();
    await wallet.switchToMainnet();
    await wallet.disconnect();

    const methods = providerRequest.mock.calls.map(([input]) => input.method);
    expect(methods).toEqual([
      "eth_requestAccounts",
      "wallet_switchEthereumChain",
      "wallet_revokePermissions",
    ]);
    expect(methods.some((method) => TRANSACTION_METHODS.has(method))).toBe(
      false,
    );
  });
});

const TRANSACTION_METHODS = new Set([
  "eth_sendTransaction",
  "eth_signTransaction",
  "wallet_sendCalls",
  "wallet_grantPermissions",
]);
