import type { AuthApi, AuthSession } from "../api/auth";
import type { WalletState } from "../app/state";

const SESSION_POLL_MS = 1_000;
const SESSION_SETTLE_GRACE_MS = 120_000;

export interface SessionWaitOptions {
  pollIntervalMs?: number;
  settleGraceMs?: number;
}

export async function waitForAuthenticatedSession(
  auth: AuthApi,
  getWalletState: () => WalletState | null,
  isWalletFlowOpen: () => boolean,
  {
    pollIntervalMs = SESSION_POLL_MS,
    settleGraceMs = SESSION_SETTLE_GRACE_MS,
  }: SessionWaitOptions = {},
): Promise<AuthSession> {
  requirePositiveDuration(pollIntervalMs, "pollIntervalMs");
  requirePositiveDuration(settleGraceMs, "settleGraceMs");
  let closedAt: number | null = null;
  for (;;) {
    const session = await auth.getSession();
    if (session !== null) {
      if (!matchesWallet(session, getWalletState())) {
        throw new Error("SIWE session does not match the connected wallet");
      }
      return session;
    }
    const wallet = getWalletState();
    if (isWalletFlowOpen()) {
      closedAt = null;
    } else if (wallet?.connected !== true) {
      throw new Error("Wallet connection was cancelled");
    } else {
      closedAt ??= Date.now();
      if (Date.now() - closedAt >= settleGraceMs) {
        throw new Error(
          "SIWE session was not created after wallet confirmation",
        );
      }
    }
    await new Promise<void>((resolve) => setTimeout(resolve, pollIntervalMs));
  }
}

function requirePositiveDuration(value: number, label: string): void {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${label}: expected positive safe integer`);
  }
}

function matchesWallet(
  session: AuthSession,
  wallet: WalletState | null,
): boolean {
  return (
    wallet?.configured === true &&
    wallet.connected &&
    Number(wallet.chainId) === 1 &&
    wallet.address?.toLowerCase() === session.address.toLowerCase()
  );
}
