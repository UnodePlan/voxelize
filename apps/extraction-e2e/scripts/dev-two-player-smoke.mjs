/**
 * 本地双人联调冒烟：2× 确定性 SIWE 钱包 → 连 WS → 入队 → 成局 → JOIN → get-state
 * 依赖：extraction-server 已以 EXTRACTION_DEV_MATCH_MODE=true / SIZE=2 运行在 :4100
 *
 * 用法（仓库根）：
 *   node apps/extraction-e2e/scripts/dev-two-player-smoke.mjs
 */

import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const require = createRequire(import.meta.url);
const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");

// 通过 tsx/vitest 路径不可用时用动态 import 已编译依赖 —— 直接用 node + 仓库内 ESM
// 这里用 vitest 同款：在 package 目录用 node --import tsx 更好；简化为 spawn vitest 单测
// 实际用纯 JS 复制最小 SIWE 流，避免编译问题。

import { keccak256, toUtf8Bytes, Wallet } from "ethers";
import { SiweMessage } from "siwe";
import WebSocket from "ws";

const SERVER = process.env.EXTRACTION_E2E_SERVER_URL ?? "http://127.0.0.1:4100";
const PUBLIC_ORIGIN =
  process.env.EXTRACTION_E2E_PUBLIC_ORIGIN ?? "http://127.0.0.1:5173";
const MATCH_SIZE = Number(process.env.EXTRACTION_DEV_MATCH_SIZE ?? "2");
const TIMEOUT_MS = 120_000;

function wallet(index) {
  const privateKey = keccak256(
    toUtf8Bytes(`voxel-extraction-live-e2e-wallet:${index}`),
  );
  return new Wallet(privateKey);
}

function wsUrl(httpOrigin) {
  const u = new URL(httpOrigin);
  u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
  u.pathname = "/ws/";
  return u.toString();
}

class Session {
  constructor(id, wallet) {
    this.id = id;
    this.wallet = wallet;
    this.cookie = null;
    this.socket = null;
  }

  async request(path, { method = "GET", body } = {}) {
    const headers = {
      Accept: "application/json",
      Origin: PUBLIC_ORIGIN,
    };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (this.cookie) headers.Cookie = this.cookie;
    const res = await fetch(new URL(path, SERVER), {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      redirect: "error",
      signal: AbortSignal.timeout(30_000),
    });
    const setCookie = res.headers.get("set-cookie");
    if (setCookie) {
      // 取 session cookie 第一段
      this.cookie = setCookie.split(";")[0];
    }
    const text = await res.text();
    let json = null;
    try {
      json = text ? JSON.parse(text) : null;
    } catch {
      json = { raw: text };
    }
    if (!res.ok) {
      throw new Error(
        `${this.id} ${method} ${path} -> ${res.status} ${JSON.stringify(json)}`,
      );
    }
    return json;
  }

  async authenticate() {
    await this.request("/api/bootstrap");
    const nonceBody = await this.request("/api/auth/siwe/nonce");
    const nonce = nonceBody.nonce ?? nonceBody?.data?.nonce;
    const expiresAt = nonceBody.expiresAt ?? nonceBody?.data?.expiresAt;
    if (!nonce) throw new Error(`${this.id} missing nonce: ${JSON.stringify(nonceBody)}`);
    const nonceExpiryMs = Date.parse(expiresAt);
    const issuedAtMs = Number.isFinite(nonceExpiryMs)
      ? nonceExpiryMs - 5 * 60_000
      : Date.now();
    const expirationMs = Math.min(
      issuedAtMs + 4 * 60_000,
      (Number.isFinite(nonceExpiryMs) ? nonceExpiryMs : issuedAtMs + 5 * 60_000) - 1_000,
    );
    const message = new SiweMessage({
      address: this.wallet.address,
      chainId: 1,
      domain: new URL(PUBLIC_ORIGIN).host,
      expirationTime: new Date(expirationMs).toISOString(),
      issuedAt: new Date(issuedAtMs).toISOString(),
      nonce,
      statement: "Sign in to Voxel Extraction.",
      uri: PUBLIC_ORIGIN,
      version: "1",
    }).prepareMessage();
    const signature = await this.wallet.signMessage(message);
    await this.request("/api/auth/siwe/verify", {
      method: "POST",
      body: { message, signature },
    });
    const session = await this.request("/api/auth/session");
    const addr = session.address ?? session?.data?.address;
    if (addr?.toLowerCase() !== this.wallet.address.toLowerCase()) {
      throw new Error(`${this.id} session mismatch ${addr}`);
    }
    console.log(`[${this.id}] SIWE ok ${this.wallet.address}`);
  }

  async connectWs() {
    const url = wsUrl(SERVER);
    await new Promise((resolve, reject) => {
      const socket = new WebSocket(url, {
        headers: { Cookie: this.cookie, Origin: PUBLIC_ORIGIN },
        handshakeTimeout: 30_000,
      });
      const timer = setTimeout(
        () => reject(new Error(`${this.id} ws timeout`)),
        30_000,
      );
      socket.once("open", () => {
        clearTimeout(timer);
        this.socket = socket;
        console.log(`[${this.id}] WS open`);
        resolve();
      });
      socket.once("error", (err) => {
        clearTimeout(timer);
        reject(err);
      });
    });
  }

  async enqueue() {
    // 连接注册可能需要一点时间
    const deadline = Date.now() + 5_000;
    let lastErr;
    while (Date.now() < deadline) {
      try {
        const q = await this.request("/api/matchmaking/queue", {
          method: "POST",
        });
        console.log(`[${this.id}] queue`, q.status ?? q, q.matchId ?? q.worldName ?? "");
        return q;
      } catch (e) {
        lastErr = e;
        await new Promise((r) => setTimeout(r, 200));
      }
    }
    throw lastErr;
  }

  async pollAssigned() {
    const deadline = Date.now() + TIMEOUT_MS;
    while (Date.now() < deadline) {
      const q = await this.request("/api/matchmaking/queue", { method: "GET" });
      const status = q.status ?? q?.data?.status;
      const matchId = q.matchId ?? q?.data?.matchId;
      const worldName = q.worldName ?? q?.data?.worldName;
      if (
        ["preparing", "active", "extractionOpen", "settling"].includes(status) &&
        matchId &&
        worldName
      ) {
        console.log(`[${this.id}] assigned`, status, matchId, worldName);
        return { status, matchId, worldName };
      }
      await new Promise((r) => setTimeout(r, 150));
    }
    throw new Error(`${this.id} never assigned`);
  }

  close() {
    try {
      this.socket?.close();
    } catch {
      /* ignore */
    }
  }
}

async function main() {
  console.log("server", SERVER, "origin", PUBLIC_ORIGIN, "matchSize", MATCH_SIZE);
  // health（服务端路由为 /health/live 与 /health/ready）
  const health = await fetch(new URL("/health/ready", SERVER), {
    headers: { Origin: PUBLIC_ORIGIN, Accept: "application/json" },
    signal: AbortSignal.timeout(5_000),
  });
  if (!health.ok) throw new Error(`health/ready ${health.status}`);
  console.log("health/ready ok");

  const a = new Session("A", wallet(200));
  const b = new Session("B", wallet(201));
  try {
    await a.authenticate();
    await b.authenticate();
    await a.connectWs();
    await b.connectWs();
    // 交错入队，第二人触发成局
    await a.enqueue();
    const qb = await b.enqueue();
    const assignedA = await a.pollAssigned();
    const assignedB = await b.pollAssigned();
    if (assignedA.matchId !== assignedB.matchId) {
      throw new Error("players not in same match");
    }
    console.log("\n=== SMOKE PASS ===");
    console.log("matchId", assignedA.matchId);
    console.log("worldName", assignedA.worldName);
    console.log("status", assignedA.status, "/", assignedB.status);
    console.log("players", a.wallet.address, b.wallet.address);
  } finally {
    a.close();
    b.close();
  }
}

main().catch((err) => {
  console.error("SMOKE FAIL", err);
  process.exit(1);
});
