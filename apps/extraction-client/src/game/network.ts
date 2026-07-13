import { protocol } from "@voxelize/protocol";

import {
  type ExtractionManifest,
  type GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

import { GameplayStateChannel } from "./gameplay-state-channel";
import { routeNetworkMessage } from "./network-message-router";
import {
  createIntent,
  requireWorldName,
  websocketUrl,
} from "./network-protocol";
import { IntentSequence } from "./network-sequence";
import { openGameSocket } from "./network-socket";
import type { GameNetworkEvents } from "./network-types";

export { createIntent, websocketUrl } from "./network-protocol";
export type { GameNetworkEvents } from "./network-types";

const RECONNECT_BASE_MS = 250;
const RECONNECT_MAX_MS = 2_000;
const RECONNECT_WINDOW_MS = 60_000;
export class GameNetwork {
  private socket: WebSocket | null = null;
  private joinedWorld: string | null = null;
  private readonly intentSequence = new IntentSequence();
  private readonly stateChannel;
  private connectPromise: Promise<void> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectExpiryTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private closedByClient = false;

  constructor(
    private readonly manifest: ExtractionManifest,
    private readonly events: GameNetworkEvents,
    private readonly serverUrl?: string,
  ) {
    this.stateChannel = new GameplayStateChannel(
      manifest,
      (name, payload) => this.sendMethod(name, payload),
      (state) => {
        this.intentSequence.seed(state);
        this.events.onGameplayState(state);
      },
    );
  }

  async connect(): Promise<void> {
    this.closedByClient = false;
    await this.ensureConnected();
    this.events.onConnection("online");
  }

  private async ensureConnected(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) return;
    if (this.connectPromise !== null) return this.connectPromise;
    const attempt = this.openSocket();
    this.connectPromise = attempt;
    try {
      await attempt;
    } finally {
      if (this.connectPromise === attempt) this.connectPromise = null;
    }
  }

  private openSocket(): Promise<void> {
    const { opened, socket } = openGameSocket(
      websocketUrl(this.serverUrl),
      (event) => this.handleMessage(event),
      (closedSocket, event) => this.handleClose(closedSocket, event),
    );
    this.socket = socket;
    return opened;
  }

  join(worldName: string): void {
    this.requireOpen();
    this.clearReconnectWindow();
    this.joinedWorld = requireWorldName(worldName);
    this.sendJoin(this.joinedWorld);
  }

  resume(worldName: string): void {
    this.requireOpen();
    this.clearReconnectWindow();
    this.joinedWorld = requireWorldName(worldName);
  }

  retryResume(): void {
    if (this.joinedWorld === null) return;
    this.closeSocketForReconnect();
  }

  private sendJoin(worldName: string): void {
    this.send({
      type: protocol.Message.Type.JOIN,
      json: JSON.stringify({
        world: worldName,
        username: "Extractor",
        preferences: {},
      }),
    });
  }

  leave(): void {
    this.clearReconnectWindow();
    if (
      this.joinedWorld !== null &&
      this.socket?.readyState === WebSocket.OPEN
    ) {
      this.send({ type: protocol.Message.Type.LEAVE, text: this.joinedWorld });
    }
    this.joinedWorld = null;
  }

  close(): void {
    this.closedByClient = true;
    this.clearReconnectWindow();
    this.joinedWorld = null;
    const socket = this.socket;
    this.socket = null;
    socket?.close(1000, "client logout");
    this.stateChannel.rejectAll("Network stopped");
    this.events.onConnection("offline");
  }

  private handleClose(socket: WebSocket, event: CloseEvent): void {
    if (this.socket !== socket) return;
    this.socket = null;
    this.stateChannel.rejectAll("WebSocket closed");
    if (event.code === 1008) {
      this.clearReconnectWindow();
      this.joinedWorld = null;
      this.events.onConnection("offline");
      this.events.onAuthenticationInvalidated();
      return;
    }
    if (this.closedByClient || this.joinedWorld === null) {
      this.events.onConnection("offline");
      return;
    }
    this.events.onConnection("reconnecting");
    this.startReconnectWindow();
    this.scheduleReconnect();
  }

  private startReconnectWindow(): void {
    if (this.reconnectExpiryTimer !== null) return;
    this.reconnectExpiryTimer = setTimeout(
      () => this.expireReconnect(),
      RECONNECT_WINDOW_MS,
    );
  }

  private scheduleReconnect(): void {
    if (
      this.closedByClient ||
      this.joinedWorld === null ||
      this.reconnectTimer !== null
    ) {
      return;
    }
    const delay = Math.min(
      RECONNECT_BASE_MS * 2 ** this.reconnectAttempts,
      RECONNECT_MAX_MS,
    );
    this.reconnectAttempts += 1;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.restoreJoinedWorld().catch(() => this.scheduleReconnect());
    }, delay);
  }

  private async restoreJoinedWorld(): Promise<void> {
    const worldName = this.joinedWorld;
    if (worldName === null || this.closedByClient) return;
    await this.ensureConnected();
    if (this.joinedWorld !== worldName || this.closedByClient) return;
    try {
      await this.requestGameplayState();
    } catch (error) {
      this.closeSocketForReconnect();
      throw error;
    }
    this.clearReconnectWindow();
    this.events.onConnection("online");
  }

  private expireReconnect(): void {
    if (this.joinedWorld === null || this.closedByClient) return;
    this.clearReconnectWindow();
    this.joinedWorld = null;
    const socket = this.socket;
    this.socket = null;
    socket?.close(1000, "reconnect window expired");
    this.stateChannel.rejectAll("Reconnect window expired");
    this.events.onConnection("offline");
    this.events.onReconnectExpired();
  }

  private clearReconnectWindow(): void {
    if (this.reconnectTimer !== null) clearTimeout(this.reconnectTimer);
    if (this.reconnectExpiryTimer !== null) {
      clearTimeout(this.reconnectExpiryTimer);
    }
    this.reconnectTimer = null;
    this.reconnectExpiryTimer = null;
    this.reconnectAttempts = 0;
  }

  requestGameplayState(): Promise<GameplayStateData> {
    this.requireOpen();
    return this.stateChannel.request();
  }

  attack(): void {
    this.sendMethod(
      "pvp:v1:attack",
      createIntent(this.manifest.protocolVersion, this.intentSequence.next(), {
        weaponSlot: "melee",
      }),
    );
  }

  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void {
    const payload = action === "start" ? { action, voxel } : { action };
    this.sendMethod(
      "pvp:v1:mining",
      createIntent(
        this.manifest.protocolVersion,
        this.intentSequence.next(),
        payload,
      ),
    );
  }

  dropSlot(slot: number, expectedInventoryRevision: number): void {
    this.sendMethod(
      "pvp:v1:drop-slot",
      createIntent(this.manifest.protocolVersion, this.intentSequence.next(), {
        slot,
        expectedInventoryRevision,
      }),
    );
  }

  private handleMessage(event: MessageEvent): void {
    try {
      const routed = routeNetworkMessage(event);
      if (routed?.kind === "error") {
        this.stateChannel.rejectAll("Server rejected the gameplay request");
        this.events.onProtocolError("服务端拒绝了实时请求");
      } else if (routed?.kind === "result") {
        this.stateChannel.handleResult(routed.value);
      } else if (routed?.kind === "state") {
        this.stateChannel.scheduleSync();
      }
    } catch {
      this.events.onProtocolError("收到无法解析的实时消息");
    }
  }

  private sendMethod(name: string, payload: unknown): void {
    this.requireOpen();
    this.send({
      type: protocol.Message.Type.METHOD,
      method: { name, payload: JSON.stringify(payload) },
    });
  }

  private send(message: protocol.IMessage): void {
    const socket = this.requireOpen();
    socket.send(
      protocol.Message.encode(protocol.Message.create(message)).finish(),
    );
  }

  private requireOpen(): WebSocket {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      throw new Error("Game WebSocket is not open");
    }
    return this.socket;
  }

  private closeSocketForReconnect(): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.close(1012, "retry authenticated rebind");
    }
  }
}
