import type { AppKit } from "@reown/appkit";
import type { SIWECreateMessageArgs, SIWESession } from "@reown/appkit-siwe";

import logoUrl from "../../../../examples/client/src/assets/logo-circle.png";
import type { AuthApi } from "../api/auth";
import { ETHEREUM_MAINNET_CHAIN_ID } from "../api/auth";
import type { WalletState } from "../app/state";

import type { LogoutCoordinator } from "./logout-coordinator";

export interface ReownWallet {
  configured: boolean;
  disconnect(): Promise<void>;
  getState(): WalletState;
  isOpen(): boolean;
  open(): Promise<void>;
  subscribe(listener: (state: WalletState) => void): () => void;
  switchToMainnet(): Promise<void>;
}

export interface CreateReownWalletOptions {
  auth: AuthApi;
  logout: LogoutCoordinator;
  projectId?: string;
}

export async function createReownWallet({
  auth,
  logout,
  projectId = import.meta.env.VITE_REOWN_PROJECT_ID,
}: CreateReownWalletOptions): Promise<ReownWallet> {
  if (projectId === undefined || projectId.trim() === "") {
    return createUnavailableWallet();
  }

  const [{ createAppKit }, { EthersAdapter }, { mainnet }, siweModule] =
    await Promise.all([
      import("@reown/appkit"),
      import("@reown/appkit-adapter-ethers"),
      import("@reown/appkit/networks"),
      import("@reown/appkit-siwe"),
    ]);
  // AppKit 可能在同一次 SIWE 流程中多次调用 getNonce；一次签名必须复用
  // 同一个服务端一次性 nonce，否则签名消息与服务端观测到的 nonce 会分叉。
  let siweNonce: string | null = null;
  let siweNonceRequest: Promise<string> | null = null;
  const siweConfig = siweModule.createSIWEConfig({
    required: true,
    signOutOnDisconnect: true,
    signOutOnAccountChange: true,
    signOutOnNetworkChange: true,
    getMessageParams: async () => {
      const issuedAt = new Date();
      return {
        domain: window.location.host,
        uri: window.location.origin,
        chains: [ETHEREUM_MAINNET_CHAIN_ID],
        statement: "Sign in to Voxel Extraction.",
        iat: issuedAt.toISOString(),
        exp: new Date(issuedAt.getTime() + 5 * 60_000).toISOString(),
      };
    },
    createMessage: ({ address, ...args }: SIWECreateMessageArgs) =>
      siweModule.formatMessage(args, address),
    getNonce: async () => {
      if (siweNonce !== null) return siweNonce;
      siweNonceRequest ??= auth.getNonce().then(({ nonce }) => {
        siweNonce = nonce;
        return nonce;
      });
      return siweNonceRequest;
    },
    getSession: async (): Promise<SIWESession | null> => auth.getSession(),
    verifyMessage: async ({ message, signature }) => {
      try {
        return (await auth.verifyMessage({ message, signature })) === true;
      } catch {
        return false;
      } finally {
        siweNonce = null;
        siweNonceRequest = null;
      }
    },
    signOut: () => logout.logout(),
  });
  const appKit = createAppKit({
    adapters: [new EthersAdapter()],
    networks: [mainnet],
    defaultNetwork: mainnet,
    projectId: projectId.trim(),
    metadata: {
      name: "Voxel Extraction",
      description: "Voxel Extraction PVP",
      url: window.location.origin,
      icons: [new URL(logoUrl, window.location.origin).toString()],
    },
    features: {
      analytics: false,
      email: false,
      onramp: false,
      receive: false,
      send: false,
      socials: [],
      swaps: false,
    },
    allowUnsupportedChain: false,
    themeMode: "dark",
    siweConfig,
  });
  return createWalletFacade(appKit, mainnet);
}

function createWalletFacade(
  appKit: AppKit,
  mainnet: Parameters<AppKit["switchNetwork"]>[0],
): ReownWallet {
  const listeners = new Set<(state: WalletState) => void>();
  const initialAccount = appKit.getAccount("eip155");
  let connected = initialAccount?.isConnected ?? false;
  let address: string | null = initialAccount?.address ?? null;
  let chainId = appKit.getChainId() ?? null;
  const getState = (): WalletState => ({
    configured: true,
    connected,
    address,
    chainId,
  });
  const emit = () => listeners.forEach((listener) => listener(getState()));
  appKit.subscribeAccount((next) => {
    connected = next.isConnected;
    address = next.address ?? null;
    emit();
  }, "eip155");
  appKit.subscribeNetwork((next) => {
    chainId = next.chainId ?? null;
    emit();
  });

  return {
    configured: true,
    disconnect: async () => {
      await appKit.disconnect("eip155");
    },
    getState,
    isOpen: () => appKit.isOpen(),
    open: async () => {
      await appKit.open();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    switchToMainnet: () =>
      appKit.switchNetwork(mainnet, { throwOnFailure: true }),
  };
}

function createUnavailableWallet(): ReownWallet {
  const state: WalletState = {
    configured: false,
    connected: false,
    address: null,
    chainId: null,
  };
  return {
    configured: false,
    disconnect: async () => undefined,
    getState: () => state,
    isOpen: () => false,
    open: async () => undefined,
    subscribe: () => () => undefined,
    switchToMainnet: async () => undefined,
  };
}
