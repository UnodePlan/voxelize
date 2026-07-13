import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import { decodeExtractionManifest } from "../../../../contracts/extraction/v1/typescript";
import type { AuthSession } from "../api/auth";
import type { ReownWallet } from "../auth/appkit";

import { ProductController } from "./controller";
import type { WalletState } from "./state";

const controllerMocks = vi.hoisted(() => {
  const auth = {
    getNonce: vi.fn(),
    getSession: vi.fn(),
    logout: vi.fn(),
    verifyMessage: vi.fn(),
  };
  const match = {
    activate: vi.fn(),
    attack: vi.fn(),
    close: vi.fn(),
    dropSlot: vi.fn(),
    joinQueue: vi.fn(),
    leaveQueue: vi.fn(),
    loadAuthenticatedSession: vi.fn(),
    loadLobby: vi.fn(),
    markWorldReady: vi.fn(),
    mining: vi.fn(),
    movement: vi.fn(),
    refreshResult: vi.fn(),
    sendWorldPacket: vi.fn(),
  };
  const scene = {
    handleNetworkMessage: vi.fn(),
    resetLiveWorld: vi.fn(),
  };
  return {
    auth,
    bindProductCommands: vi.fn(),
    commandError: vi.fn(() => "command failed"),
    fetchExtractionManifest: vi.fn(),
    match,
    MatchCoordinator: vi.fn(function () {
      return match;
    }),
    mountProductShell: vi.fn(() => ({ canvas: {}, shell: {} })),
    renderControllerState: vi.fn(),
    scene,
    VoxelBackdrop: vi.fn(function () {
      return scene;
    }),
  };
});

vi.mock("../api", () => ({
  fetchExtractionManifest: controllerMocks.fetchExtractionManifest,
}));
vi.mock("../api/auth", () => ({
  createAuthApi: vi.fn(() => controllerMocks.auth),
}));
vi.mock("../game/scene", () => ({
  VoxelBackdrop: controllerMocks.VoxelBackdrop,
}));
vi.mock("../ui/shell", () => ({
  mountProductShell: controllerMocks.mountProductShell,
}));
vi.mock("./commands", () => ({
  bindProductCommands: controllerMocks.bindProductCommands,
  commandError: controllerMocks.commandError,
}));
vi.mock("./match-coordinator", () => ({
  MatchCoordinator: controllerMocks.MatchCoordinator,
}));
vi.mock("./render", () => ({
  renderControllerState: controllerMocks.renderControllerState,
}));

const manifest = decodeExtractionManifest(manifestJson);
const session: AuthSession = {
  address: "0x1111111111111111111111111111111111111111",
  chainId: 1,
};

describe("ProductController 钱包身份边界", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    controllerMocks.fetchExtractionManifest.mockResolvedValue(manifest);
    controllerMocks.auth.getSession.mockResolvedValue(session);
    controllerMocks.auth.logout.mockResolvedValue(undefined);
    controllerMocks.match.loadAuthenticatedSession.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it.each([
    {
      name: "账号变化",
      state: {
        configured: true,
        connected: true,
        address: "0x2222222222222222222222222222222222222222",
        chainId: 1,
      },
    },
    {
      name: "钱包断连",
      state: {
        configured: true,
        connected: false,
        address: null,
        chainId: 1,
      },
    },
    {
      name: "错误链切换",
      state: {
        configured: true,
        connected: true,
        address: session.address,
        chainId: 11_155_111,
      },
    },
  ])("$name 会注销会话并关闭比赛运行时", async ({ state }) => {
    const wallet = walletHarness();
    const controller = new ProductController(
      {} as HTMLElement,
      vi.fn(async () => wallet.value),
    );
    await controller.start();
    expect(controllerMocks.match.activate).toHaveBeenCalledOnce();

    wallet.emit(state);

    expect(controllerMocks.auth.logout).toHaveBeenCalledOnce();
    expect(controllerMocks.match.close).toHaveBeenCalledOnce();
    expect(wallet.value.disconnect).not.toHaveBeenCalled();
    expect(controllerMocks.renderControllerState).toHaveBeenLastCalledWith(
      expect.objectContaining({
        connection: "offline",
        screen: "unauthenticated",
        session: null,
      }),
      expect.anything(),
      expect.anything(),
    );
  });
});

function walletHarness(): {
  emit(state: WalletState): void;
  value: ReownWallet;
} {
  let onStateChange: ((state: WalletState) => void) | null = null;
  const value: ReownWallet = {
    configured: true,
    disconnect: vi.fn(async () => undefined),
    getState: () => ({
      configured: true,
      connected: true,
      address: session.address,
      chainId: 1,
    }),
    isOpen: () => false,
    open: vi.fn(async () => undefined),
    subscribe: vi.fn((listener) => {
      onStateChange = listener;
      return () => {
        onStateChange = null;
      };
    }),
    switchToMainnet: vi.fn(async () => undefined),
  };
  return {
    emit: (state) => {
      if (onStateChange === null) throw new Error("wallet listener missing");
      onStateChange(state);
    },
    value,
  };
}
