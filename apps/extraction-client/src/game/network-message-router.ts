import type { MessageProtocol } from "@voxelize/protocol";

import { isStateMethod } from "./network-protocol";

const RESULT_METHOD = "pvp:v1:result";

export type RoutedNetworkMessage =
  | { kind: "error" }
  | { kind: "result"; value: unknown }
  | { kind: "state" }
  | { kind: "voxel"; message: MessageProtocol };

export function routeNetworkMessage(
  message: MessageProtocol,
): RoutedNetworkMessage | null {
  if (message.type === "ERROR") return { kind: "error" };
  const method = message.method;
  if (
    message.type !== "METHOD" ||
    method === null ||
    method === undefined ||
    typeof method.name !== "string" ||
    typeof method.payload !== "string"
  ) {
    return { kind: "voxel", message };
  }
  if (method.name === RESULT_METHOD) {
    return { kind: "result", value: JSON.parse(method.payload) as unknown };
  }
  return isStateMethod(method.name) ? { kind: "state" } : null;
}
