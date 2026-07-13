import { afterEach, describe, expect, it, vi } from "vitest";

import type { AuthApi } from "../api/auth";

import { createReownWallet } from "./appkit";
import { LogoutCoordinator } from "./logout-coordinator";

const appKitMocks = vi.hoisted(() => {
  type AccountListener = (state: {
    address?: string;
    isConnected: boolean;
  }) => void;
  type NetworkListener = (state: { chainId?: number }) => void;

  let siweConfig: Record<string, (...args: never[]) => unknown> | null = null;
  let accountListener: AccountListener | null = null;
  let networkListener: NetworkListener | null = null;
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
    subscribeAccount: vi.fn((listener: AccountListener) => {
      accountListener = listener;
    }),
    subscribeNetwork: vi.fn((listener: NetworkListener) => {
      networkListener = listener;
    }),
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
    emitAccount: (state: Parameters<AccountListener>[0]) => {
      if (accountListener === null) throw new Error("account listener missing");
      accountListener(state);
    },
    emitNetwork: (state: Parameters<NetworkListener>[0]) => {
      if (networkListener === null) throw new Error("network listener missing");
      networkListener(state);
    },
    resetSubscriptions: () => {
      accountListener = null;
      networkListener = null;
      siweConfig = null;
    },
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
    appKitMocks.resetSubscriptions();
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
      logout: vi.fn(async () => undefined),
      verifyMessage: vi.fn(async () => true),
    } as unknown as AuthApi;
    const logout = new LogoutCoordinator(auth);

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

  it.each([
    {
      name: "账号变化",
      emit: () =>
        appKitMocks.emitAccount({
          address: "0x2222222222222222222222222222222222222222",
          isConnected: true,
        }),
      expected: {
        connected: true,
        address: "0x2222222222222222222222222222222222222222",
        chainId: 1,
      },
    },
    {
      name: "钱包断连",
      emit: () =>
        appKitMocks.emitAccount({ address: undefined, isConnected: false }),
      expected: { connected: false, address: null, chainId: 1 },
    },
    {
      name: "错误链切换",
      emit: () => appKitMocks.emitNetwork({ chainId: 11_155_111 }),
      expected: {
        connected: true,
        address: "0x1111111111111111111111111111111111111111",
        chainId: 11_155_111,
      },
    },
  ])("$name 会通过 AppKit 订阅发出新状态", async ({ emit, expected }) => {
    stubBrowser();
    const auth = createAuthMock();
    const logout = new LogoutCoordinator(auth);
    const wallet = await createReownWallet({
      auth,
      logout,
      projectId: "test-project-id",
    });
    const onStateChange = vi.fn();
    wallet.subscribe(onStateChange);

    emit();

    expect(onStateChange).toHaveBeenCalledOnce();
    expect(onStateChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ configured: true, ...expected }),
    );
  });
});

function stubBrowser(): void {
  vi.stubGlobal("ethereum", { request: vi.fn(async () => null) });
  vi.stubGlobal("window", {
    location: {
      host: "game.example.test",
      origin: "https://game.example.test",
    },
  });
}

function createAuthMock(): AuthApi {
  return {
    getNonce: vi.fn(),
    getSession: vi.fn(),
    logout: vi.fn(async () => undefined),
    verifyMessage: vi.fn(),
  } as unknown as AuthApi;
}

const TRANSACTION_METHODS = new Set([
  "eth_sendTransaction",
  "eth_signTransaction",
  "wallet_sendCalls",
  "wallet_grantPermissions",
]);
