import { ProductController } from "../app/controller";
import type { ReownWallet } from "../auth/appkit";

const WALLET_STORAGE_KEY = "__VOXEL_EXTRACTION_LIVE_E2E__";
const ADDRESS_PATTERN = /^0x[0-9a-f]{40}$/iu;

export function startLiveE2eClient(root: HTMLElement): Promise<void> {
  const controller = new ProductController(root, createLiveWallet);
  return controller.start();
}

async function createLiveWallet(): Promise<ReownWallet> {
  const address = localStorage.getItem(WALLET_STORAGE_KEY);
  if (address === null || !ADDRESS_PATTERN.test(address)) {
    throw new Error("live E2E wallet address is missing");
  }
  const state = {
    configured: true,
    connected: true,
    address,
    chainId: 1,
  } as const;
  return {
    configured: true,
    disconnect: async () => undefined,
    getState: () => state,
    isOpen: () => false,
    open: async () => undefined,
    subscribe: (listener) => {
      listener(state);
      return () => undefined;
    },
    switchToMainnet: async () => undefined,
  };
}
