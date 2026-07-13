import { createRequire } from "node:module";

import type { RawData } from "ws";

type ProtocolModule = typeof import("@voxelize/protocol");
type Lz4Module = {
  decompress(input: Uint8Array): ArrayLike<number>;
};

const require = createRequire(import.meta.url);
// protocol 的 ESM 产物当前不能直接在 Node 加载；CJS 导出与浏览器使用同一生成协议。
const { protocol } = require("@voxelize/protocol") as ProtocolModule;
const lz4 = require("lz4js") as Lz4Module;
type MessageInput = Parameters<typeof protocol.Message.create>[0];

const LZ4_FRAME_MAGIC = [0x04, 0x22, 0x4d, 0x18] as const;
const SERVER_MESSAGE_TYPES = new Set([
  "INIT",
  "JOIN",
  "LEAVE",
  "ERROR",
  "PEER",
  "ENTITY",
  "LOAD",
  "UNLOAD",
  "UPDATE",
  "METHOD",
  "CHAT",
  "TRANSPORT",
  "EVENT",
  "ACTION",
  "STATS",
]);

export interface LiveServerFrame {
  entities: LiveEntity[];
  json: unknown;
  method: LiveMethod | null;
  peers: LivePeer[];
  text: string;
  type: string;
  worldName: string;
}

export interface LiveEntity {
  id: string;
  metadata: unknown;
  operation: "CREATE" | "DELETE" | "UPDATE";
  type: string;
}

export interface LiveMethod {
  name: string;
  payload: unknown;
}

export interface LivePeer {
  id: string;
  metadata: unknown;
  username: string;
}

export interface LiveMovementInput {
  direction: [number, number, number];
  movement: { forward: number; jump: boolean; right: number };
}

export function encodeJoin(worldName: string): Uint8Array {
  requireWorldName(worldName);
  return protocol.Message.encode(
    protocol.Message.create({
      type: protocol.Message.Type.JOIN,
      json: JSON.stringify({
        world: worldName,
        username: "Extraction E2E",
        preferences: {},
      }),
    }),
  ).finish();
}

export function encodeMethod(name: string, payload: unknown): Uint8Array {
  if (name.trim() === "" || name.length > 128) {
    throw new Error("live method name is invalid");
  }
  return encodeMessage({
    type: protocol.Message.Type.METHOD,
    method: { name, payload: JSON.stringify(payload) },
  });
}

export function encodeMovement(input: LiveMovementInput): Uint8Array {
  return encodeMessage({
    type: protocol.Message.Type.PEER,
    peers: [
      {
        id: "",
        username: "Extraction E2E",
        metadata: JSON.stringify(input),
      },
    ],
  });
}

export function decodeServerFrame(input: Uint8Array): LiveServerFrame {
  const bytes = isLz4Frame(input)
    ? Uint8Array.from(lz4.decompress(input))
    : input;
  const message = protocol.Message.decode(bytes);
  const type = protocol.Message.Type[message.type];
  if (typeof type !== "string" || !SERVER_MESSAGE_TYPES.has(type)) {
    throw new Error("live server frame has an unknown message type");
  }
  const worldName = message.worldName ?? "";
  if (type === "INIT") requireWorldName(worldName);
  const entities = (message.entities ?? []).map((entity) => {
    const decodedOperation =
      typeof entity.operation === "number"
        ? protocol.Entity.Operation[entity.operation]
        : undefined;
    if (
      decodedOperation !== "CREATE" &&
      decodedOperation !== "DELETE" &&
      decodedOperation !== "UPDATE"
    ) {
      throw new Error("live entity has an unknown operation");
    }
    const operation: LiveEntity["operation"] = decodedOperation;
    return {
      id: entity.id ?? "",
      metadata: parseOptionalJson(entity.metadata, "entity metadata"),
      operation,
      type: entity.type ?? "",
    };
  });
  const method = message.method
    ? {
        name: message.method.name ?? "",
        payload: parseOptionalJson(message.method.payload, "method payload"),
      }
    : null;
  const peers = (message.peers ?? []).map((peer) => ({
    id: peer.id ?? "",
    metadata: parseOptionalJson(peer.metadata, "peer metadata"),
    username: peer.username ?? "",
  }));
  return {
    entities,
    json: parseOptionalJson(message.json, "message json"),
    method,
    peers,
    type,
    text: message.text ?? "",
    worldName,
  };
}

export function requireWorldName(value: string): string {
  if (value.trim() === "" || value.length > 128) {
    throw new Error("live worldName is invalid");
  }
  return value;
}

export function rawDataToBytes(data: RawData): Uint8Array {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (!Array.isArray(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  const total = data.reduce((sum, part) => sum + part.byteLength, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const part of data) {
    result.set(part, offset);
    offset += part.byteLength;
  }
  return result;
}

function isLz4Frame(bytes: Uint8Array): boolean {
  return LZ4_FRAME_MAGIC.every((value, index) => bytes[index] === value);
}

function encodeMessage(message: MessageInput): Uint8Array {
  return protocol.Message.encode(protocol.Message.create(message)).finish();
}

function parseOptionalJson(
  value: string | null | undefined,
  label: string,
): unknown {
  if (value == null || value === "") return null;
  try {
    return JSON.parse(value) as unknown;
  } catch (cause) {
    throw new Error(`live ${label} is not valid JSON`, { cause });
  }
}
