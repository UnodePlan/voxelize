import type { AuthSession } from "../api/auth";
import type { WalletState } from "../app/state";

const RESTORE_GRACE_MS = 1_500;
export type WalletIdentityStatus = "invalid" | "pending" | "valid";

export class WalletIdentityGuard {
  private timer: ReturnType<typeof setTimeout> | null = null;

  validate(
    wallet: WalletState,
    session: AuthSession,
    invalidate: () => void,
  ): WalletIdentityStatus {
    if (matchesSession(wallet, session)) {
      this.clear();
      return "valid";
    }
    if (!wallet.configured || wallet.connected) {
      this.clear();
      invalidate();
      return "invalid";
    }
    this.timer ??= setTimeout(() => {
      this.timer = null;
      invalidate();
    }, RESTORE_GRACE_MS);
    return "pending";
  }

  clear(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }
}

function matchesSession(wallet: WalletState, session: AuthSession): boolean {
  return (
    wallet.configured &&
    wallet.connected &&
    Number(wallet.chainId) === 1 &&
    wallet.address?.toLowerCase() === session.address.toLowerCase()
  );
}
