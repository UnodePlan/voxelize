import type { AuthSession } from "../api/auth";
import type { WalletState } from "../app/state";

const RESTORE_GRACE_MS = 1_500;
export type WalletIdentityStatus = "invalid" | "pending" | "valid";

export class WalletIdentityGuard {
  // 宽限仅用于页面启动恢复；已建立身份后的任何不匹配都必须立即注销。
  private identityWasValid = false;
  private timer: ReturnType<typeof setTimeout> | null = null;

  validate(
    wallet: WalletState,
    session: AuthSession,
    invalidate: () => void,
  ): WalletIdentityStatus {
    if (matchesSession(wallet, session)) {
      this.cancelTimer();
      this.identityWasValid = true;
      return "valid";
    }
    if (!wallet.configured || wallet.connected || this.identityWasValid) {
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
    this.cancelTimer();
    this.identityWasValid = false;
  }

  private cancelTimer(): void {
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
