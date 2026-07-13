import { protocol } from "@voxelize/protocol";
import type { MessageProtocol } from "@voxelize/protocol";

import {
  type ExtractionManifest,
  type GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

import { GameplayStateChannel } from "./gameplay-state-channel";
import { GameplayNetworkEgress } from "./network-egress";
import type { MovementInput } from "./network-egress";
import { routeNetworkMessage } from "./network-message-router";
import { requireWorldName, websocketUrl } from "./network-protocol";
import { reconnectDelay, startReconnectExpiry } from "./network-reconnect";
import { openGameSocket } from "./network-socket";
import type { GameNetworkEvents } from "./network-types";
import { ProtocolDecoder } from "./protocol-decoder";

export { createIntent, websocketUrl } from "./network-protocol";
export type { GameNetworkEvents } from "./network-types";
export type { MovementInput } from "./network-egress";

export class GameNetwork {
  private socket: WebSocket | null = null;
  private joinedWorld: string | null = null;
  private readonly stateChannel;
  private readonly egress;
  private connectPromise: Promise<void> | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectExpiryTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  private closedByClient = false;
  private readonly decoder;

  constructor(
    manifest: ExtractionManifest,
    private readonly events: GameNetworkEvents,
    private readonly serverUrl?: string,
  ) {
    this.egress = new GameplayNetworkEgress(
      manifest.protocolVersion,
      (message) => this.send(message),
    );
    this.decoder = new ProtocolDecoder(
      (message) => this.handleDecodedMessage(message),
      () => {
        this.events.onProtocolError("收到无法解析的体素世界消息");
        if (this.joinedWorld !== null) this.closeSocketForReconnect();
      },
    );
    this.stateChannel = new GameplayStateChannel(
      manifest,
      (name, payload) => this.egress.sendMethod(name, payload),
      (state) => {
        this.egress.seed(state);
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
    if (this.joinedWorld !== null) this.events.onVoxelReset?.();
    this.decoder.reset();
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
    this.decoder.reset();
    this.events.onVoxelReset?.();
  }

  close(): void {
    this.closedByClient = true;
    this.clearReconnectWindow();
    this.joinedWorld = null;
    const socket = this.socket;
    this.socket = null;
    socket?.close(1000, "client logout");
    this.stateChannel.rejectAll("Network stopped");
    this.decoder.dispose();
    this.events.onVoxelReset?.();
    this.events.onConnection("offline");
  }

  private handleClose(socket: WebSocket, event: CloseEvent): void {
    if (this.socket !== socket) return;
    this.socket = null;
    this.stateChannel.rejectAll("WebSocket closed");
    if (event.code === 1008) {
      this.clearReconnectWindow();
      this.joinedWorld = null;
      this.decoder.reset();
      this.events.onVoxelReset?.();
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
    this.reconnectExpiryTimer = startReconnectExpiry(() =>
      this.expireReconnect(),
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
    const delay = reconnectDelay(this.reconnectAttempts);
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
    this.decoder.reset();
    this.events.onVoxelReset?.();
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
    this.requireOpen();
    this.egress.attack();
  }

  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void {
    this.requireOpen();
    this.egress.mining(action, voxel);
  }

  dropSlot(slot: number, expectedInventoryRevision: number): void {
    this.requireOpen();
    this.egress.dropSlot(slot, expectedInventoryRevision);
  }

  movement(input: MovementInput): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.egress.movement(input);
  }

  sendWorldPacket(message: MessageProtocol): void {
    if (this.socket?.readyState !== WebSocket.OPEN) return;
    this.egress.sendWorldPacket(message);
  }

  private handleMessage(event: MessageEvent): void {
    if (event.data instanceof ArrayBuffer) this.decoder.decode(event.data);
  }

  private handleDecodedMessage(message: MessageProtocol): void {
    try {
      const routed = routeNetworkMessage(message);
      if (routed?.kind === "error") {
        this.stateChannel.rejectAll("Server rejected the gameplay request");
        this.events.onProtocolError("服务端拒绝了实时请求");
      } else if (routed?.kind === "result") {
        this.stateChannel.handleResult(routed.value);
      } else if (routed?.kind === "state") {
        this.stateChannel.scheduleSync();
      } else if (routed?.kind === "voxel" && this.joinedWorld !== null) {
        this.events.onVoxelMessage?.(routed.message);
      }
    } catch {
      this.events.onProtocolError("收到无法解析的实时消息");
    }
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
