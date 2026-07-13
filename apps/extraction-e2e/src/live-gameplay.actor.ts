import { EventEmitter } from "node:events";
import { createRequire } from "node:module";

import { describe, expect, it, vi } from "vitest";
import type WebSocket from "ws";

import manifestJson from "../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  type GameplayStateData,
} from "../../../contracts/extraction/v1/typescript";

import { lookAt } from "./live/gameplay-actions";
import { GameplayProtocolDriver } from "./live/gameplay-driver";
import { GameplaySocketClosedError } from "./live/gameplay-errors";
import { GameplayFrameStore } from "./live/gameplay-frame-store";
import { fetchLiveMatchResult } from "./live/gameplay-http";
import {
  inventoryResourceCount,
  supportedSurfaceVoxelTowardCenter,
  surfaceVoxelBelow,
  visibleTopFacePoint,
} from "./live/gameplay-state";
import type { LiveHttpTransport } from "./live/http";
import {
  decodeServerFrame,
  encodeMethod,
  encodeMovement,
  type LiveServerFrame,
} from "./live/wire";

type ProtocolModule = typeof import("@voxelize/protocol");
const require = createRequire(import.meta.url);
const { protocol } = require("@voxelize/protocol") as ProtocolModule;
const manifest = decodeExtractionManifest(manifestJson);
const matchId = "00000000-0000-4000-8000-000000000011";

describe("live gameplay protobuf driver", () => {
  it("encodes only the production PEER and METHOD intent shapes", () => {
    const movement = decodeServerFrame(
      encodeMovement({
        direction: [1, 0, 0],
        movement: { forward: 1, jump: false, right: 0 },
      }),
    );
    expect(movement.type).toBe("PEER");
    expect(movement.peers).toEqual([
      {
        id: "",
        username: "Extraction E2E",
        metadata: {
          direction: [1, 0, 0],
          movement: { forward: 1, jump: false, right: 0 },
        },
      },
    ]);

    const method = decodeServerFrame(
      encodeMethod("pvp:v1:attack", { sequence: 7 }),
    );
    expect(method.type).toBe("METHOD");
    expect(method.method).toEqual({
      name: "pvp:v1:attack",
      payload: { sequence: 7 },
    });
  });

  it("decodes INIT/PEER positions and correlates get-state before gameplay sequence", async () => {
    const socket = new FakeSocket();
    const driver = new GameplayProtocolDriver(
      socket as unknown as WebSocket,
      initFrame(),
      matchId,
      manifest,
      1_000,
    );
    expect(driver.position()).toEqual([125, 50, 0]);

    const stateRequest = driver.getState();
    const getState = decodeServerFrame(socket.sent[0]);
    const getStateEnvelope = getState.method?.payload as {
      requestId: string;
      sequence: number;
    };
    expect(getState.method?.name).toBe("pvp:v1:get-state");
    socket.receive(resultFrame(getStateEnvelope.requestId, gameplayState()));
    expect((await stateRequest).attack.acceptedSequence).toBe(9);

    const attackRequest = driver.attack();
    const attack = decodeServerFrame(socket.sent[1]);
    const attackEnvelope = attack.method?.payload as {
      requestId: string;
      sequence: number;
    };
    expect(attackEnvelope.sequence).toBe(10);
    socket.receive(
      resultFrame(attackEnvelope.requestId, {
        acceptedSequence: 10,
        attackRevision: 10,
        resolution: "miss",
      }),
    );
    await expect(attackRequest).resolves.toMatchObject({
      acceptedSequence: 10,
      resolution: "miss",
    });
    const pendingState = {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "extraction",
      revision: 6,
      data: {
        status: "pending",
        zone: { center: [0, 50, 0], radiusBlocks: 4, halfHeightBlocks: 3 },
        qualifiedAtUnixSeconds: 1_800_000_488,
      },
    };
    socket.receive(methodFrame("pvp:v1:extraction-state", pendingState));
    socket.emit("close");
    await expect(
      driver.waitForMethodState(
        "pvp:v1:extraction-state",
        (value) => value as typeof pendingState,
        (value) => value.data.status === "pending",
        "pending state missing",
      ),
    ).resolves.toEqual(pendingState);
    driver.dispose();
  });

  it("waits for the authoritative look direction before advancing action time", async () => {
    const socket = new FakeSocket();
    const driver = new GameplayProtocolDriver(
      socket as unknown as WebSocket,
      initFrame(),
      matchId,
      manifest,
      1_000,
    );
    const elapsed: number[] = [];
    // 初始缓存方向已经是 -X，仍必须等待本次发送后的新 PEER revision。
    const looking = lookAt(driver, [124, 50, 0], {
      async elapse(milliseconds) {
        elapsed.push(milliseconds);
        return { monotonicMs: null };
      },
    });

    await Promise.resolve();
    expect(socket.sent).toHaveLength(1);
    expect(elapsed).toEqual([]);
    socket.receive(peerFrame([-1, 0, 0], [125, 49.5, 0]));

    for (let sentCount = 2; sentCount <= 4; sentCount += 1) {
      await vi.waitFor(() => expect(socket.sent).toHaveLength(sentCount));
      expect(elapsed).toEqual([]);
      const corrected = decodeServerFrame(socket.sent[sentCount - 1]).peers[0]
        .metadata as { direction: [number, number, number] };
      socket.receive(peerFrame(corrected.direction, [125, 49.5, 0]));
    }

    await expect(looking).resolves.toBeUndefined();
    expect(elapsed).toEqual([50]);
    driver.dispose();
  });

  it("treats an HTTP 200 null match result as not-yet-persisted", async () => {
    const transport: LiveHttpTransport = {
      async request() {
        return { body: null, setCookie: null, status: 200 };
      },
    };
    await expect(fetchLiveMatchResult(transport, matchId)).resolves.toBeNull();
  });

  it("classifies a close before terminal state as an explicit socket close", async () => {
    const socket = new FakeSocket();
    const driver = new GameplayProtocolDriver(
      socket as unknown as WebSocket,
      initFrame(),
      matchId,
      manifest,
      1_000,
    );
    const terminal = driver.waitForMethodState(
      "pvp:v1:extraction-state",
      (value) => value,
      () => false,
      "terminal state missing",
    );

    socket.emit("close");

    await expect(terminal).rejects.toBeInstanceOf(GameplaySocketClosedError);
    driver.dispose();
  });

  it("terminates the transport to enter the real detach path", async () => {
    const socket = new FakeSocket();
    const driver = new GameplayProtocolDriver(
      socket as unknown as WebSocket,
      initFrame(),
      matchId,
      manifest,
      1_000,
    );

    await driver.disconnectTransport();

    expect(socket.terminated).toBe(true);
    await expect(driver.getState()).rejects.toBeInstanceOf(
      GameplaySocketClosedError,
    );
    driver.dispose();
  });

  it("tracks one projected loot lifecycle and deterministic surface inventory helpers", () => {
    const store = new GameplayFrameStore(initFrame());
    store.apply(
      frame({
        entities: [
          {
            id: "drop:v1:test",
            metadata: { loot: { contents: { dirt: 1 } } },
            operation: "CREATE",
            type: "extraction:loot",
          },
        ],
        type: "ENTITY",
      }),
    );
    expect(store.visibleLoot()).toHaveLength(1);
    expect(store.lootCreationCount("drop:v1:test")).toBe(1);
    store.apply(
      frame({
        entities: [
          {
            id: "drop:v1:test",
            metadata: null,
            operation: "DELETE",
            type: "",
          },
        ],
        type: "ENTITY",
      }),
    );
    expect(store.observedLoot()).toEqual(["drop:v1:test"]);
    expect(store.removedLoot()).toEqual(["drop:v1:test"]);
    expect(surfaceVoxelBelow([125.25, 50, -0.25])).toEqual([125, 48, -1]);
    expect(supportedSurfaceVoxelTowardCenter([125.25, 50, -0.25])).toEqual([
      123, 48, -1,
    ]);
    expect(supportedSurfaceVoxelTowardCenter([1.25, 50, -8.25])).toEqual([
      1, 48, -7,
    ]);
    expect(visibleTopFacePoint([123, 48, -1])).toEqual([123.5, 48.999, -0.5]);
    expect(inventoryResourceCount(gameplayState(), "dirt")).toBe(1);
  });

  it("counts duplicate loot CREATE frames even when their ID is unchanged", () => {
    const store = new GameplayFrameStore(initFrame());
    const create = frame({
      entities: [
        {
          id: "drop:v1:duplicate",
          metadata: { loot: { contents: { dirt: 1 } } },
          operation: "CREATE",
          type: "extraction:loot",
        },
      ],
      type: "ENTITY",
    });
    store.apply(create);
    store.apply(create);
    expect(store.observedLoot()).toEqual(["drop:v1:duplicate"]);
    expect(store.lootCreationCount("drop:v1:duplicate")).toBe(2);
  });
});

class FakeSocket extends EventEmitter {
  readonly sent: Uint8Array[] = [];
  readyState = 1;
  terminated = false;

  send(
    data: Uint8Array,
    _options: { binary: boolean },
    callback: (error?: Error) => void,
  ): void {
    this.sent.push(Uint8Array.from(data));
    callback();
  }

  receive(data: Uint8Array): void {
    this.emit("message", Buffer.from(data), true);
  }

  terminate(): void {
    this.terminated = true;
    this.readyState = 3;
    this.emit("close");
  }
}

function initFrame(): LiveServerFrame {
  return frame({
    json: { id: "player-1", savedPosition: [125, 50, 0] },
    peers: [
      {
        id: "player-1",
        metadata: { position: [125, 50, 0], direction: [-1, 0, 0] },
        username: "Extractor",
      },
    ],
    type: "INIT",
    worldName: "match-test",
  });
}

function frame(overrides: Partial<LiveServerFrame>): LiveServerFrame {
  return {
    entities: [],
    json: null,
    method: null,
    peers: [],
    text: "",
    type: "PEER",
    worldName: "",
    ...overrides,
  };
}

function resultFrame(requestId: string, data: unknown): Uint8Array {
  return methodFrame("pvp:v1:result", {
    protocolVersion: 1,
    type: "result",
    requestId,
    outcome: { status: "ok", data },
  });
}

function methodFrame(name: string, payload: unknown): Uint8Array {
  return protocol.Message.encode(
    protocol.Message.create({
      type: protocol.Message.Type.METHOD,
      method: {
        name,
        payload: JSON.stringify(payload),
      },
    }),
  ).finish();
}

function peerFrame(
  direction: [number, number, number],
  position: [number, number, number],
): Uint8Array {
  return protocol.Message.encode(
    protocol.Message.create({
      type: protocol.Message.Type.PEER,
      peers: [
        {
          id: "player-1",
          username: "Extractor",
          metadata: JSON.stringify({ position, direction }),
        },
      ],
    }),
  ).finish();
}

function gameplayState(): GameplayStateData {
  const inventory = {
    slots: [
      { resource: "dirt" as const, quantity: 1 },
      ...Array(11).fill(null),
    ],
    revision: 1,
    frozen: false,
    lastDropSequence: 7,
  };
  return {
    matchId,
    inventory,
    equipment: {
      pickaxe: "basic_pickaxe",
      meleeWeapon: "basic_melee_weapon",
    },
    mining: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "mining",
      revision: 7,
      data: { status: "idle", acceptedSequence: 7, reason: "initial" },
    },
    extraction: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "extraction",
      revision: 0,
      data: {
        status: "hidden",
        extractionOpenAtUnixSeconds: 1_800_000_480,
        hardDeadlineUnixSeconds: 1_800_000_720,
      },
    },
    health: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "health",
      revision: 0,
      data: { status: "alive", currentHalfHearts: 20, maxHalfHearts: 20 },
    },
    attack: { revision: 9, acceptedSequence: 9 },
    deathResult: null,
  };
}
