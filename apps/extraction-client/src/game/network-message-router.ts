import { protocol } from "@voxelize/protocol";

import { isCompressed, isStateMethod } from "./network-protocol";

const RESULT_METHOD = "pvp:v1:result";

export type RoutedNetworkMessage =
  | { kind: "error" }
  | { kind: "result"; value: unknown }
  | { kind: "state" };

export function routeNetworkMessage(
  event: MessageEvent,
): RoutedNetworkMessage | null {
  if (!(event.data instanceof ArrayBuffer)) return null;
  const bytes = new Uint8Array(event.data);
  if (isCompressed(bytes)) return null;
  const message = protocol.Message.decode(bytes);
  if (message.type === protocol.Message.Type.ERROR) return { kind: "error" };
  const method = message.method;
  if (
    message.type !== protocol.Message.Type.METHOD ||
    method === null ||
    method === undefined ||
    typeof method.name !== "string" ||
    typeof method.payload !== "string"
  ) {
    return null;
  }
  if (method.name === RESULT_METHOD) {
    return { kind: "result", value: JSON.parse(method.payload) as unknown };
  }
  return isStateMethod(method.name) ? { kind: "state" } : null;
}
