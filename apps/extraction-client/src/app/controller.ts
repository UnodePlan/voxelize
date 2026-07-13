import { fetchExtractionManifest } from "../api";
import { createAuthApi, type AuthSession } from "../api/auth";
import { createReownWallet, type ReownWallet } from "../auth/appkit";
import { LogoutCoordinator } from "../auth/logout-coordinator";
import { waitForAuthenticatedSession } from "../auth/wait-for-session";
import {
  WalletIdentityGuard,
  type WalletIdentityStatus,
} from "../auth/wallet-identity";
import { VoxelBackdrop } from "../game/scene";
import { mountProductShell } from "../ui/shell";

import { bindProductCommands, commandError } from "./commands";
import { MatchCoordinator } from "./match-coordinator";
import { renderControllerState } from "./render";
import {
  INITIAL_APP_STATE,
  reduceAppState,
  type AppAction,
  type AppState,
  type WalletState,
} from "./state";

export class ProductController {
  private state: AppState = INITIAL_APP_STATE;
  private readonly elements;
  private readonly scene;
  private readonly auth = createAuthApi();
  private readonly logout = new LogoutCoordinator(this.auth);
  private readonly walletIdentity = new WalletIdentityGuard();
  private readonly match;
  private wallet: ReownWallet | null = null;

  constructor(root: HTMLElement) {
    this.elements = mountProductShell(root);
    this.scene = new VoxelBackdrop(this.elements.canvas);
    this.match = new MatchCoordinator({
      dispatch: (action) => this.dispatch(action),
      getState: () => this.state,
      onAuthenticationInvalidated: () => void this.endSession(false),
    });
    bindProductCommands(this.elements.shell, {
      commands: {
        "connect-wallet": () => this.connectWallet(),
        logout: () => this.endSession(true),
        "join-queue": () => this.match.joinQueue(),
        "leave-queue": () => this.match.leaveQueue(),
        "refresh-lobby": () => this.match.loadLobby(),
        "back-lobby": () => this.match.loadLobby(),
        "show-result": () => this.showResult(),
        "refresh-result": () => this.match.refreshResult(),
        "retry-bootstrap": () => window.location.reload(),
      },
      onBusy: (operation) => this.dispatch({ type: "BUSY", operation }),
      onError: (operation) =>
        this.dispatch({ type: "NOTICE", message: commandError(operation) }),
    });
    setInterval(() => {
      if (this.state.screen === "match") this.render();
    }, 1_000);
  }

  async start(): Promise<void> {
    this.render();
    try {
      const [manifest, session] = await Promise.all([
        fetchExtractionManifest(),
        this.auth.getSession(),
      ]);
      this.dispatch({ type: "BOOTSTRAP_READY", manifest, session });
      if (session !== null) this.logout.activate();
      this.wallet = await createReownWallet({
        auth: this.auth,
        logout: this.logout,
      });
      const identityStatus = this.attachWallet(this.wallet);
      if (session !== null && identityStatus !== "invalid") {
        this.match.activate();
        await this.match.loadAuthenticatedSession();
      }
    } catch {
      this.dispatch({ type: "BOOTSTRAP_FAILED", message: "游戏服务暂不可用" });
      if (this.wallet === null) {
        this.wallet = await createReownWallet({
          auth: this.auth,
          logout: this.logout,
        });
        this.attachWallet(this.wallet);
      }
    }
  }

  private attachWallet(wallet: ReownWallet): WalletIdentityStatus | null {
    wallet.subscribe((next) => {
      this.handleWallet(next);
    });
    return this.handleWallet(wallet.getState());
  }

  private showResult(): void {
    if (this.state.result !== null) {
      this.dispatch({ type: "MATCH_RESULT", result: this.state.result });
    }
  }

  private async connectWallet(): Promise<void> {
    if (this.wallet === null || !this.wallet.configured) {
      throw new Error("wallet unavailable");
    }
    if (
      this.state.wallet.connected &&
      Number(this.state.wallet.chainId) !== 1
    ) {
      await this.wallet.switchToMainnet();
    } else {
      await this.wallet.open();
    }
    let session: AuthSession;
    try {
      session = await waitForAuthenticatedSession(
        this.auth,
        () => this.wallet?.getState() ?? null,
        () => this.wallet?.isOpen() ?? false,
      );
      if (
        this.walletIdentity.validate(
          this.wallet.getState(),
          session,
          () => undefined,
        ) !== "valid"
      ) {
        throw new Error("wallet identity changed during SIWE");
      }
    } catch (error) {
      await this.endSession(false);
      throw error;
    }
    this.logout.activate();
    this.dispatch({ type: "SESSION_READY", session });
    this.match.activate();
    await this.match.loadAuthenticatedSession();
  }

  private handleWallet(wallet: WalletState): WalletIdentityStatus | null {
    this.dispatch({
      type: "WALLET_CHANGED",
      connected: wallet.connected,
      address: wallet.address ?? undefined,
      chainId: wallet.chainId ?? undefined,
    });
    this.dispatch({ type: "WALLET_CONFIGURED", configured: wallet.configured });
    const session = this.state.session;
    if (session === null) return null;
    return this.walletIdentity.validate(
      wallet,
      session,
      () => void this.endSession(false),
    );
  }

  private async endSession(disconnectWallet: boolean): Promise<void> {
    const logout = this.logout.logout();
    this.walletIdentity.clear();
    this.match.close();
    this.dispatch({ type: "SESSION_CLEARED" });
    if (disconnectWallet) await this.wallet?.disconnect();
    if (!(await logout)) {
      this.dispatch({ type: "NOTICE", message: "服务端登出待确认" });
    }
  }

  private dispatch(action: AppAction): void {
    this.state = reduceAppState(this.state, action);
    this.render();
  }

  private render(): void {
    renderControllerState(this.state, this.elements, this.scene);
  }
}
