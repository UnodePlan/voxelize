/**
 * DEV 多人联调：确定性 EOA + 真实 SIWE，无需 MetaMask UI。
 * 仅 import.meta.env.DEV 入口使用。
 */

import { keccak256, toUtf8Bytes, Wallet } from "ethers";
import { SiweMessage } from "siwe";

import { createAuthApi, ETHEREUM_MAINNET_CHAIN_ID } from "../api/auth";
import type { ReownWallet } from "../auth/appkit";
import type { WalletState } from "../app/state";

const NONCE_TTL_MS = 5 * 60_000;
const MESSAGE_LIFETIME_MS = 4 * 60_000;

/** 与 extraction-e2e live 钱包派生一致，仅用于无资产本地身份 */
export function deterministicDevWallet(seat: number): Wallet {
  if (!Number.isInteger(seat) || seat < 0 || seat > 31) {
    throw new Error("dev-mp seat 须为 0–31 整数");
  }
  const privateKey = keccak256(
    toUtf8Bytes(`voxel-extraction-live-e2e-wallet:${seat}`),
  );
  return new Wallet(privateKey);
}

export function parseDevMpSeat(
  search: string = window.location.search,
): number {
  const raw = new URLSearchParams(search).get("seat");
  if (raw === null || raw === "") return 0;
  const seat = Number(raw);
  if (!Number.isInteger(seat) || seat < 0 || seat > 31) {
    throw new Error("?seat= 须为 0–31 整数");
  }
  return seat;
}

/** 完成 SIWE 会话 cookie；返回地址 */
export async function authenticateDevSeat(seat: number): Promise<string> {
  const wallet = deterministicDevWallet(seat);
  const auth = createAuthApi();
  const { nonce, expiresAt } = await auth.getNonce();
  const message = buildSiweMessage(wallet.address, nonce, expiresAt);
  const signature = await wallet.signMessage(message);
  await auth.verifyMessage({ message, signature });
  const session = await auth.getSession();
  if (
    session === null ||
    session.address.toLowerCase() !== wallet.address.toLowerCase()
  ) {
    throw new Error("DEV SIWE 会话校验失败");
  }
  return session.address;
}

/** 已登录态的假钱包（跳过 Reown 弹窗） */
export function createDevConnectedWallet(address: string): ReownWallet {
  const state: WalletState = {
    configured: true,
    connected: true,
    address,
    chainId: ETHEREUM_MAINNET_CHAIN_ID,
  };
  const listeners = new Set<(next: WalletState) => void>();
  return {
    configured: true,
    disconnect: async () => {
      /* 联调会话不主动清 cookie；刷新即可换 seat */
    },
    getState: () => state,
    isOpen: () => false,
    open: async () => undefined,
    subscribe: (listener) => {
      listeners.add(listener);
      listener(state);
      return () => {
        listeners.delete(listener);
      };
    },
    switchToMainnet: async () => undefined,
  };
}

function buildSiweMessage(
  address: string,
  nonce: string,
  expiresAt: string,
): string {
  const nonceExpiryMs = Date.parse(expiresAt);
  const issuedAtMs = Number.isFinite(nonceExpiryMs)
    ? nonceExpiryMs - NONCE_TTL_MS
    : Date.now();
  const expirationMs = Math.min(
    issuedAtMs + MESSAGE_LIFETIME_MS,
    (Number.isFinite(nonceExpiryMs) ? nonceExpiryMs : issuedAtMs + NONCE_TTL_MS) -
      1_000,
  );
  return new SiweMessage({
    address,
    chainId: ETHEREUM_MAINNET_CHAIN_ID,
    domain: window.location.host,
    expirationTime: new Date(expirationMs).toISOString(),
    issuedAt: new Date(issuedAtMs).toISOString(),
    nonce,
    statement: "Sign in to Voxel Extraction.",
    uri: window.location.origin,
    version: "1",
  }).prepareMessage();
}
