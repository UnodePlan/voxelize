import type { Wallet } from "ethers";
import WebSocket, { type RawData } from "ws";

import { decodeExtractionManifest } from "../../../../contracts/extraction/v1/manifest";
import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";
import type {
  CapacityActor,
  CapacityAdmission,
  CapacityJoin,
} from "../capacity-scenario";

import type { LiveE2eConfig } from "./config";
import { GameplayProtocolDriver } from "./gameplay-driver";
import {
  CookieHttpTransport,
  requestJson,
  type LiveHttpTransport,
} from "./http";
import { enqueueProtocolActor, type PrimaryAdmissionBarrier } from "./queue";
import { authenticateEoa } from "./siwe";
import {
  decodeServerFrame,
  encodeJoin,
  rawDataToBytes,
  type LiveServerFrame,
} from "./wire";

export interface ProtocolGameplaySession {
  driver: GameplayProtocolDriver;
  http: LiveHttpTransport;
}

export class ProtocolCapacityActor implements CapacityActor {
  readonly kind = "protocol" as const;
  private readonly http;
  private socket: WebSocket | null = null;
  private admission: CapacityAdmission | null = null;
  private driver: GameplayProtocolDriver | null = null;
  private manifest: ExtractionManifest | null = null;

  constructor(
    readonly actorId: string,
    private readonly wallet: Wallet,
    private readonly primary: boolean,
    private readonly barrier: PrimaryAdmissionBarrier,
    private readonly config: LiveE2eConfig,
  ) {
    this.http = new CookieHttpTransport(
      config.serverUrl,
      config.publicOrigin,
      config.scenarioTimeoutMs,
    );
  }

  async connect(): Promise<void> {
    if (this.socket !== null)
      throw new Error(`${this.actorId} already connected`);
    this.manifest = await requestJson(
      this.http,
      "/api/bootstrap",
      { method: "GET" },
      decodeExtractionManifest,
    );
    await authenticateEoa(this.http, this.wallet, this.config);
    const socket = new WebSocket(this.config.protocolWebSocketUrl, {
      handshakeTimeout: this.config.scenarioTimeoutMs,
      headers: { Cookie: this.http.sessionCookie },
      origin: this.config.publicOrigin,
    });
    this.socket = socket;
    await waitForOpen(socket, this.config.scenarioTimeoutMs, this.actorId);
  }

  async enqueue(): Promise<CapacityAdmission> {
    this.requireOpenSocket();
    this.admission = await enqueueProtocolActor(
      this.actorId,
      this.primary,
      this.http,
      this.barrier,
      this.config,
    );
    return this.admission;
  }

  async join(matchId: string, worldName: string): Promise<CapacityJoin> {
    if (this.admission === null) {
      throw new Error(`${this.actorId} attempted JOIN before admission`);
    }
    const outcome = await waitForJoinOutcome(
      this.requireOpenSocket(),
      matchId,
      worldName,
      this.config.scenarioTimeoutMs,
    );
    if (outcome.result.status === "joined" && outcome.init !== null) {
      const manifest = this.manifest;
      if (manifest === null) throw new Error("live manifest is unavailable");
      this.driver = new GameplayProtocolDriver(
        this.requireOpenSocket(),
        outcome.init,
        matchId,
        manifest,
        this.config.scenarioTimeoutMs,
      );
    }
    return outcome.result;
  }

  gameplaySession(): ProtocolGameplaySession {
    if (this.driver === null) {
      throw new Error(`${this.actorId} has not joined a gameplay World`);
    }
    return { driver: this.driver, http: this.http };
  }

  async disconnect(): Promise<void> {
    const socket = this.socket;
    this.socket = null;
    this.driver?.dispose();
    this.driver = null;
    if (
      socket === null ||
      socket.readyState === WebSocket.CLOSED ||
      socket.readyState === WebSocket.CLOSING
    ) {
      return;
    }
    await closeSocket(socket, Math.min(this.config.connectionGraceMs, 2_000));
  }

  private requireOpenSocket(): WebSocket {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      throw new Error(`${this.actorId} WebSocket is not open`);
    }
    return this.socket;
  }
}

function waitForOpen(
  socket: WebSocket,
  timeoutMs: number,
  actorId: string,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => finish(new Error(`${actorId} WebSocket open timed out`)),
      timeoutMs,
    );
    const onOpen = () => finish();
    const onError = (error: Error) => finish(error);
    const onClose = () => finish(new Error(`${actorId} WebSocket closed`));
    const cleanup = () => {
      clearTimeout(timer);
      socket.off("open", onOpen);
      socket.off("error", onError);
      socket.off("close", onClose);
    };
    const finish = (error?: Error) => {
      cleanup();
      if (error === undefined) resolve();
      else reject(error);
    };
    socket.once("open", onOpen);
    socket.once("error", onError);
    socket.once("close", onClose);
  });
}

function waitForJoinOutcome(
  socket: WebSocket,
  matchId: string,
  worldName: string,
  timeoutMs: number,
): Promise<{ init: LiveServerFrame | null; result: CapacityJoin }> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => finish(new Error("timed out waiting for INIT or ERROR")),
      timeoutMs,
    );
    const onMessage = (data: RawData, isBinary: boolean) => {
      if (!isBinary) return finish(new Error("server sent a text frame"));
      let frame;
      try {
        frame = decodeServerFrame(rawDataToBytes(data));
      } catch (error) {
        return finish(
          error instanceof Error ? error : new Error("decode failed"),
        );
      }
      if (frame.type === "INIT") {
        if (frame.worldName !== worldName) {
          return finish(new Error("INIT returned the wrong world"));
        }
        return finish(undefined, {
          init: frame,
          result: { status: "joined", matchId, worldName },
        });
      }
      if (frame.type === "ERROR") {
        const code = classifyJoinErrorText(frame.text);
        if (code === null) {
          return finish(
            new Error(`server returned unexpected JOIN error: ${frame.text}`),
          );
        }
        return finish(undefined, {
          init: null,
          result: { status: "rejected", code },
        });
      }
    };
    const onError = (error: Error) => finish(error);
    const onClose = () => {
      finish(new Error("WebSocket closed before explicit JOIN result"));
    };
    const cleanup = () => {
      clearTimeout(timer);
      socket.off("message", onMessage);
      socket.off("error", onError);
      socket.off("close", onClose);
    };
    const finish = (
      error?: Error,
      result?: { init: LiveServerFrame | null; result: CapacityJoin },
    ) => {
      cleanup();
      if (error !== undefined) reject(error);
      else if (result !== undefined) resolve(result);
      else reject(new Error("JOIN completed without a result"));
    };
    socket.on("message", onMessage);
    socket.once("error", onError);
    socket.once("close", onClose);
    socket.send(encodeJoin(worldName), { binary: true }, (error) => {
      if (error != null) finish(error);
    });
  });
}

export function classifyJoinErrorText(
  text: string,
): "MATCH_FULL" | "MATCH_ROSTER_LOCKED" | null {
  if (text === "World is full.") return "MATCH_FULL";
  if (
    text === "Client admission was denied." ||
    text === "World is not accepting clients."
  ) {
    return "MATCH_ROSTER_LOCKED";
  }
  return null;
}

function closeSocket(socket: WebSocket, timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      socket.terminate();
      resolve();
    }, timeoutMs);
    socket.once("close", () => {
      clearTimeout(timer);
      resolve();
    });
    socket.close(1000, "capacity scenario complete");
  });
}
