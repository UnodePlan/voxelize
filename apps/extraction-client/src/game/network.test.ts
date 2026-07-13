import { protocol } from "@voxelize/protocol";
import type { MessageProtocol } from "@voxelize/protocol";
import { afterEach, describe, expect, it, vi } from "vitest";

import getStateFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import { decodeExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { createIntent, GameNetwork, websocketUrl } from "./network";

const manifest = decodeExtractionManifest(manifestJson);

describe("game network boundary", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    FakeWebSocket.instances = [];
  });

  it("uses the authenticated same-origin websocket path", () => {
    vi.stubGlobal("window", { location: { origin: "https://play.example" } });
    expect(websocketUrl("")).toBe("wss://play.example/ws/");
    expect(websocketUrl("http://127.0.0.1:4100/api")).toBe(
      "ws://127.0.0.1:4100/ws/",
    );
  });

  it("creates versioned intents without client-authoritative combat fields", () => {
    vi.stubGlobal("crypto", {
      randomUUID: () => "11111111-1111-4111-8111-111111111111",
    });
    const intent = createIntent(1, 9, { weaponSlot: "melee" });
    expect(intent).toEqual({
      protocolVersion: 1,
      type: "intent",
      requestId: "11111111-1111-4111-8111-111111111111",
      sequence: 9,
      payload: { weaponSlot: "melee" },
    });
    expect(intent.payload).not.toHaveProperty("damage");
    expect(intent.payload).not.toHaveProperty("targetId");
  });

  it("single-flights connects and restores full state after server rebind", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const connections: string[] = [];
    const snapshots = vi.fn();
    const reconnectExpired = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: (state) => connections.push(state),
        onGameplayState: snapshots,
        onProtocolError: vi.fn(),
        onReconnectExpired: reconnectExpired,
      },
      "https://play.example",
    );

    const firstConnect = network.connect();
    const secondConnect = network.connect();
    expect(FakeWebSocket.instances).toHaveLength(1);
    FakeWebSocket.instances[0]?.open();
    await Promise.all([firstConnect, secondConnect]);
    network.join("match:v1:test-world");

    FakeWebSocket.instances[0]?.serverClose();
    expect(connections.at(-1)).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(250);
    expect(FakeWebSocket.instances).toHaveLength(2);
    FakeWebSocket.instances[1]?.open();
    await vi.advanceTimersByTimeAsync(0);

    const restored = decodedMessages(FakeWebSocket.instances[1]);
    expect(restored).toHaveLength(1);
    expect(restored[0]).toMatchObject({
      type: protocol.Message.Type.METHOD,
      method: { name: "pvp:v1:get-state" },
    });
    const request = JSON.parse(restored[0]?.method?.payload ?? "null") as {
      requestId: string;
    };
    const gameplayState = getStateFixtures.cases.find(
      ({ name }) => name === "valid-alive-gameplay-state",
    )?.value;
    FakeWebSocket.instances[1]?.receive({
      type: protocol.Message.Type.METHOD,
      method: {
        name: "pvp:v1:result",
        payload: JSON.stringify({
          protocolVersion: manifest.protocolVersion,
          type: "result",
          requestId: request.requestId,
          outcome: { status: "ok", data: gameplayState },
        }),
      },
    });
    await vi.advanceTimersByTimeAsync(0);

    expect(snapshots).toHaveBeenCalledWith(
      expect.objectContaining({
        inventory: expect.objectContaining({ revision: 4 }),
      }),
    );
    expect(connections.at(-1)).toBe("online");
    expect(reconnectExpired).not.toHaveBeenCalled();

    network.attack();
    network.mining("cancel");
    network.dropSlot(0, 4);
    expect(
      decodedMessages(FakeWebSocket.instances[1]).slice(1).map(intentSequence),
    ).toEqual([18, 19, 20]);

    network.close();
  });

  it("restores the authoritative intent cursor after a fresh page load", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const protocolError = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: protocolError,
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;
    network.resume("match:v1:reload-test");

    const stateRequest = network.requestGameplayState();
    const socket = FakeWebSocket.instances[0];
    const request = decodedMessages(socket).at(-1);
    const requestId = JSON.parse(request?.method?.payload ?? "null") as {
      requestId: string;
    };
    sendGameplayState(socket, requestId.requestId, validGameplayState());
    await expect(stateRequest).resolves.toMatchObject({
      inventory: { lastDropSequence: 17 },
    });

    const delayedStateRequest = network.requestGameplayState();
    const delayedRequest = decodedMessages(socket).at(-1);
    const delayedRequestId = JSON.parse(
      delayedRequest?.method?.payload ?? "null",
    ) as { requestId: string };
    network.attack();
    sendGameplayState(socket, delayedRequestId.requestId, validGameplayState());
    await delayedStateRequest;
    network.mining("cancel");
    expect(
      decodedMessages(socket)
        .filter(({ method }) =>
          ["pvp:v1:attack", "pvp:v1:mining"].includes(method?.name ?? ""),
        )
        .map(intentSequence),
    ).toEqual([18, 19]);

    const exhaustionRequest = network.requestGameplayState();
    const exhaustionMessage = decodedMessages(socket).at(-1);
    const exhaustionRequestId = JSON.parse(
      exhaustionMessage?.method?.payload ?? "null",
    ) as { requestId: string };
    const exhausted = validGameplayState();
    exhausted.inventory.lastDropSequence = 4_294_967_295;
    sendGameplayState(socket, exhaustionRequestId.requestId, exhausted);
    await exhaustionRequest;
    expect(() => network.dropSlot(0, 4)).toThrow("Intent sequence exhausted");
    expect(protocolError).not.toHaveBeenCalled();
    network.close();
  });

  it("rejects an invalid authoritative cursor without reseeding", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const protocolError = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: protocolError,
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;

    const stateRequest = network.requestGameplayState();
    const socket = FakeWebSocket.instances[0];
    const request = decodedMessages(socket).at(-1);
    const requestId = JSON.parse(request?.method?.payload ?? "null") as {
      requestId: string;
    };
    const invalid = structuredClone(validGameplayState());
    invalid.inventory.lastDropSequence = 4_294_967_296;
    sendGameplayState(socket, requestId.requestId, invalid);

    await expect(stateRequest).rejects.toThrow("unsigned 32-bit integer");
    expect(protocolError).toHaveBeenCalledTimes(1);
    network.close();
  });

  it("stops retrying and reports recovery after the 60 second window", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const reconnectExpired = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: reconnectExpired,
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;
    network.join("match:v1:expiry-test");
    FakeWebSocket.instances[0]?.serverClose();

    await vi.advanceTimersByTimeAsync(59_999);
    expect(reconnectExpired).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(reconnectExpired).toHaveBeenCalledTimes(1);
    const attemptsAtExpiry = FakeWebSocket.instances.length;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(FakeWebSocket.instances).toHaveLength(attemptsAtExpiry);

    network.close();
  });

  it("treats a policy close as authentication invalidation without retrying", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const invalidated = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: invalidated,
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;
    network.join("match:v1:policy-test");

    FakeWebSocket.instances[0]?.serverClose(1008);
    await vi.advanceTimersByTimeAsync(5_000);
    expect(invalidated).toHaveBeenCalledTimes(1);
    expect(FakeWebSocket.instances).toHaveLength(1);

    network.close();
  });

  it("closes without sending LEAVE so logout follows server detach rules", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;
    network.join("match:v1:detach-test");
    const messagesBeforeClose = decodedMessages(FakeWebSocket.instances[0]);

    network.close();
    expect(decodedMessages(FakeWebSocket.instances[0])).toEqual(
      messagesBeforeClose,
    );
  });

  it("rejects pending state immediately when the server sends ERROR", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const protocolError = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: protocolError,
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    FakeWebSocket.instances[0]?.open();
    await connected;
    network.resume("match:v1:error-test");
    const request = network.requestGameplayState();

    FakeWebSocket.instances[0]?.receive({
      type: protocol.Message.Type.ERROR,
      text: "rebind rejected",
    });
    await expect(request).rejects.toThrow("Server rejected");
    expect(protocolError).toHaveBeenCalledTimes(1);
    network.close();
  });

  it("routes decoded world messages and only emits approved world packets", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const voxelMessage = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
        onVoxelMessage: voxelMessage,
      },
      "https://play.example",
    );
    const connected = network.connect();
    const socket = FakeWebSocket.instances[0];
    socket?.open();
    await connected;
    network.resume("match:v1:route-test");

    socket?.receive({
      type: protocol.Message.Type.INIT,
      json: JSON.stringify({ id: "player-1" }),
    });
    await Promise.resolve();
    expect(voxelMessage).toHaveBeenCalledWith(
      expect.objectContaining({ type: "INIT", json: { id: "player-1" } }),
    );

    network.sendWorldPacket({
      type: "LOAD",
      json: { center: [0, 0], radius: 6 },
    } as MessageProtocol);
    network.sendWorldPacket({
      type: "UPDATE",
      json: { voxel: [0, 0, 0], id: 999 },
    } as MessageProtocol);
    network.movement({
      direction: [0, 0, -1],
      movement: { forward: 1, right: 0, jump: false },
    });

    const outbound = decodedMessages(socket);
    expect(outbound.map(({ type }) => type)).toEqual([
      protocol.Message.Type.LOAD,
      protocol.Message.Type.PEER,
    ]);
    expect(JSON.parse(outbound[1]?.peers[0]?.metadata ?? "null")).toEqual({
      direction: [0, 0, -1],
      movement: { forward: 1, right: 0, jump: false },
    });
    network.close();
  });

  it("rebinds instead of stalling after a malformed world packet", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const protocolError = vi.fn();
    const connections: string[] = [];
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: (state) => connections.push(state),
        onGameplayState: vi.fn(),
        onProtocolError: protocolError,
        onReconnectExpired: vi.fn(),
      },
      "https://play.example",
    );
    const connected = network.connect();
    const socket = FakeWebSocket.instances[0];
    socket?.open();
    await connected;
    network.join("match:v1:malformed-test");

    socket?.receiveBytes(new Uint8Array([0x12, 0xff]));
    await vi.advanceTimersByTimeAsync(0);

    expect(protocolError).toHaveBeenCalledTimes(1);
    expect(connections.at(-1)).toBe("reconnecting");
    network.close();
  });

  it("drops decoded messages that finish after leaving the world", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const voxelMessage = vi.fn();
    const voxelReset = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
        onVoxelMessage: voxelMessage,
        onVoxelReset: voxelReset,
      },
      "https://play.example",
    );
    const connected = network.connect();
    const socket = FakeWebSocket.instances[0];
    socket?.open();
    await connected;
    network.join("match:v1:leave-generation-test");

    socket?.receive({
      type: protocol.Message.Type.INIT,
      json: JSON.stringify({ id: "late-player" }),
    });
    network.leave();
    await Promise.resolve();

    expect(voxelMessage).not.toHaveBeenCalled();
    expect(voxelReset).toHaveBeenCalledTimes(1);
    network.close();
  });

  it("drops world messages received by the lobby socket after leaving", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const voxelMessage = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
        onVoxelMessage: voxelMessage,
      },
      "https://play.example",
    );
    const connected = network.connect();
    const socket = FakeWebSocket.instances[0];
    socket?.open();
    await connected;
    network.join("match:v1:late-world-frame-test");
    network.leave();

    socket?.receive({
      type: protocol.Message.Type.INIT,
      json: JSON.stringify({ id: "late-player" }),
    });
    await Promise.resolve();

    expect(voxelMessage).not.toHaveBeenCalled();
    network.close();
  });

  it("leaves one world and reuses the open socket for the next round", async () => {
    vi.stubGlobal("WebSocket", FakeWebSocket);
    const voxelReset = vi.fn();
    const network = new GameNetwork(
      manifest,
      {
        onAuthenticationInvalidated: vi.fn(),
        onConnection: vi.fn(),
        onGameplayState: vi.fn(),
        onProtocolError: vi.fn(),
        onReconnectExpired: vi.fn(),
        onVoxelReset: voxelReset,
      },
      "https://play.example",
    );
    const connected = network.connect();
    const socket = FakeWebSocket.instances[0];
    socket?.open();
    await connected;
    network.join("match:v1:first-round");

    network.leave();

    expect(voxelReset).toHaveBeenCalledTimes(1);
    expect(socket?.readyState).toBe(FakeWebSocket.OPEN);
    expect(decodedMessages(socket).at(-1)).toMatchObject({
      type: protocol.Message.Type.LEAVE,
      text: "match:v1:first-round",
    });

    network.join("match:v1:second-round");
    expect(FakeWebSocket.instances).toHaveLength(1);
    expect(decodedMessages(socket).at(-1)).toMatchObject({
      type: protocol.Message.Type.JOIN,
    });
    network.close();
  });
});

function decodedMessages(socket: FakeWebSocket | undefined) {
  return (socket?.sent ?? []).map((bytes) =>
    protocol.Message.decode(new Uint8Array(bytes)),
  );
}

function intentSequence(message: protocol.Message | undefined): number {
  return (
    JSON.parse(message?.method?.payload ?? "null") as { sequence: number }
  ).sequence;
}

function validGameplayState() {
  const state = getStateFixtures.cases.find(
    ({ name }) => name === "valid-alive-gameplay-state",
  )?.value;
  if (state === undefined) throw new Error("missing gameplay state fixture");
  return structuredClone(state);
}

function sendGameplayState(
  socket: FakeWebSocket | undefined,
  requestId: string,
  state: ReturnType<typeof validGameplayState>,
): void {
  socket?.receive({
    type: protocol.Message.Type.METHOD,
    method: {
      name: "pvp:v1:result",
      payload: JSON.stringify({
        protocolVersion: manifest.protocolVersion,
        type: "result",
        requestId,
        outcome: { status: "ok", data: state },
      }),
    },
  });
}

class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  binaryType: BinaryType = "blob";
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onopen: ((event: Event) => void) | null = null;
  readyState = FakeWebSocket.CONNECTING;
  readonly sent: Uint8Array[] = [];

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }

  open(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.(new Event("open"));
  }

  send(data: ArrayBuffer | ArrayBufferView): void {
    const view = data instanceof ArrayBuffer ? new Uint8Array(data) : data;
    this.sent.push(
      new Uint8Array(view.buffer, view.byteOffset, view.byteLength).slice(),
    );
  }

  receive(message: protocol.IMessage): void {
    const bytes = protocol.Message.encode(
      protocol.Message.create(message),
    ).finish();
    this.receiveBytes(bytes);
  }

  receiveBytes(bytes: Uint8Array): void {
    this.onmessage?.({ data: Uint8Array.from(bytes).buffer } as MessageEvent);
  }

  close(): void {
    if (this.readyState === FakeWebSocket.CLOSED) return;
    this.serverClose();
  }

  serverClose(code = 1006): void {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.(new CloseEvent("close", { code }));
  }
}
