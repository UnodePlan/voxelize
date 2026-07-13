import type { Wallet } from "ethers";
import WebSocket, { type RawData } from "ws";

import { decodeExtractionManifest } from "../../../../contracts/extraction/v1/manifest";
import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import type { LiveE2eConfig } from "./config";
import { GameplayProtocolDriver } from "./gameplay-driver";
import {
  CookieHttpTransport,
  requestJson,
  type LiveHttpTransport,
} from "./http";
import { classifyJoinErrorText } from "./protocol-actor";
import { authenticateEoa } from "./siwe";
import {
  decodeServerFrame,
  encodeJoin,
  rawDataToBytes,
  type LiveServerFrame,
} from "./wire";

export type ReconnectJoinRejection = "MATCH_FULL" | "MATCH_ROSTER_LOCKED";

export interface ReconnectedGameplaySession {
  driver: GameplayProtocolDriver;
  http: LiveHttpTransport;
}

type SocketOutcome =
  | { kind: "rebound"; init: LiveServerFrame }
  | { kind: "rejected"; code: ReconnectJoinRejection };

/** 使用真实 SIWE 和 WebSocket 验证自动 rebind，不发送第二次 JOIN。 */
export class ReconnectProtocolActor {
  private readonly http: CookieHttpTransport;
  private driver: GameplayProtocolDriver | null = null;
  private manifest: ExtractionManifest | null = null;
  private socket: WebSocket | null = null;

  constructor(
    readonly actorId: string,
    private readonly wallet: Wallet,
    private readonly config: LiveE2eConfig,
  ) {
    this.http = new CookieHttpTransport(
      config.serverUrl,
      config.publicOrigin,
      config.scenarioTimeoutMs,
    );
  }

  async automaticRebind(
    matchId: string,
    worldName: string,
  ): Promise<ReconnectedGameplaySession> {
    await this.authenticate();
    const socket = this.openSocket();
    const outcome = await waitForSocketOutcome(
      socket,
      worldName,
      this.config.scenarioTimeoutMs,
      this.actorId,
      null,
    );
    if (outcome.kind !== "rebound") {
      throw new Error(`${this.actorId} automatic rebind was rejected`);
    }
    const manifest = this.manifest;
    if (manifest === null) throw new Error("reconnect manifest is unavailable");
    this.driver = new GameplayProtocolDriver(
      socket,
      outcome.init,
      matchId,
      manifest,
      this.config.scenarioTimeoutMs,
    );
    return { driver: this.driver, http: this.http };
  }

  async expectJoinRejected(
    worldName: string,
    claimedPlayerId: string,
  ): Promise<ReconnectJoinRejection> {
    await this.authenticate();
    const socket = this.openSocket(claimedPlayerId);
    const outcome = await waitForSocketOutcome(
      socket,
      worldName,
      this.config.scenarioTimeoutMs,
      this.actorId,
      encodeJoin(worldName),
    );
    if (outcome.kind !== "rejected") {
      throw new Error(`${this.actorId} unexpectedly took over a match seat`);
    }
    return outcome.code;
  }

  authenticatedHttp(): LiveHttpTransport {
    if (this.manifest === null) {
      throw new Error(`${this.actorId} has not authenticated`);
    }
    return this.http;
  }

  async disconnect(): Promise<void> {
    const socket = this.socket;
    this.socket = null;
    this.driver?.dispose();
    this.driver = null;
    if (socket !== null) await closeSocket(socket);
  }

  private async authenticate(): Promise<void> {
    if (this.manifest !== null) return;
    this.manifest = await requestJson(
      this.http,
      "/api/bootstrap",
      { method: "GET" },
      decodeExtractionManifest,
    );
    await authenticateEoa(this.http, this.wallet, this.config);
  }

  private openSocket(claimedPlayerId?: string): WebSocket {
    if (this.socket !== null) throw new Error(`${this.actorId} already opened`);
    const url = new URL(this.config.protocolWebSocketUrl);
    if (claimedPlayerId !== undefined) {
      url.searchParams.set("client_id", claimedPlayerId);
    }
    const socket = new WebSocket(url, {
      handshakeTimeout: this.config.scenarioTimeoutMs,
      headers: { Cookie: this.http.sessionCookie },
      origin: this.config.publicOrigin,
    });
    this.socket = socket;
    return socket;
  }
}

function waitForSocketOutcome(
  socket: WebSocket,
  worldName: string,
  timeoutMs: number,
  actorId: string,
  join: Uint8Array | null,
): Promise<SocketOutcome> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => finish(new Error(`${actorId} reconnect outcome timed out`)),
      timeoutMs,
    );
    const onOpen = () => {
      if (join === null) return;
      socket.send(join, { binary: true }, (error) => {
        if (error != null) finish(error);
      });
    };
    const onMessage = (data: RawData, isBinary: boolean) => {
      if (!isBinary) return finish(new Error("server sent a text frame"));
      let frame: LiveServerFrame;
      try {
        frame = decodeServerFrame(rawDataToBytes(data));
      } catch (error) {
        return finish(
          error instanceof Error ? error : new Error("reconnect decode failed"),
        );
      }
      if (frame.type === "INIT") {
        if (frame.worldName !== worldName) {
          return finish(new Error("reconnect INIT returned another world"));
        }
        if (join !== null) {
          return finish(new Error("rejected account received a match INIT"));
        }
        return finish(undefined, { kind: "rebound", init: frame });
      }
      if (frame.type !== "ERROR") return;
      const code = classifyJoinErrorText(frame.text);
      if (join === null || code === null) {
        return finish(
          new Error(`unexpected reconnect error frame: ${frame.text}`),
        );
      }
      return finish(undefined, { kind: "rejected", code });
    };
    const onError = (error: Error) => finish(error);
    const onClose = () =>
      finish(new Error(`${actorId} socket closed before reconnect outcome`));
    const cleanup = () => {
      clearTimeout(timer);
      socket.off("open", onOpen);
      socket.off("message", onMessage);
      socket.off("error", onError);
      socket.off("close", onClose);
    };
    const finish = (error?: Error, outcome?: SocketOutcome) => {
      cleanup();
      if (error !== undefined) reject(error);
      else if (outcome !== undefined) resolve(outcome);
      else reject(new Error("reconnect socket completed without an outcome"));
    };
    socket.once("open", onOpen);
    socket.on("message", onMessage);
    socket.once("error", onError);
    socket.once("close", onClose);
  });
}

function closeSocket(socket: WebSocket): Promise<void> {
  if (socket.readyState === WebSocket.CLOSED) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      socket.terminate();
      resolve();
    }, 2_000);
    socket.once("close", () => {
      clearTimeout(timer);
      resolve();
    });
    if (socket.readyState === WebSocket.OPEN) {
      socket.close(1000, "reconnect scenario complete");
    } else {
      socket.terminate();
    }
  });
}
