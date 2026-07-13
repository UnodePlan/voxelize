import { apiBaseUrl } from "../api";

const STATE_METHODS = new Set([
  "pvp:v1:inventory-state",
  "pvp:v1:mining-state",
  "pvp:v1:extraction-state",
  "pvp:v1:health-state",
  "pvp:v1:death-result",
]);

export function websocketUrl(
  baseUrl: string | undefined = apiBaseUrl(),
): string {
  const url = new URL(baseUrl === "" ? window.location.origin : baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/ws/";
  url.search = "";
  url.hash = "";
  return url.toString();
}

export function createIntent(
  protocolVersion: number,
  sequence: number,
  payload: Record<string, unknown>,
): {
  protocolVersion: number;
  type: "intent";
  requestId: string;
  sequence: number;
  payload: Record<string, unknown>;
} {
  return {
    protocolVersion,
    type: "intent",
    requestId: crypto.randomUUID(),
    sequence,
    payload,
  };
}

export function isCompressed(bytes: Uint8Array): boolean {
  return (
    bytes.length >= 4 &&
    bytes[0] === 0x04 &&
    bytes[1] === 0x22 &&
    bytes[2] === 0x4d &&
    bytes[3] === 0x18
  );
}

export function isStateMethod(value: string): boolean {
  return STATE_METHODS.has(value);
}

export function requireWorldName(value: string): string {
  if (value.trim() === "" || value.length > 128) {
    throw new Error("worldName: invalid value");
  }
  return value;
}
