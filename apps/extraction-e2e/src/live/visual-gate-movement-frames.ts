import { decodeServerFrame } from "./wire";

export interface OutgoingMovementFrame {
  direction: [number, number, number];
  forward: number;
}

export function readOutgoingMovement(
  bytes: Uint8Array,
): OutgoingMovementFrame[] {
  try {
    const frame = decodeServerFrame(bytes);
    return frame.peers.flatMap((peer) => {
      const metadata = peer.metadata;
      if (metadata === null || typeof metadata !== "object") return [];
      const movement = (metadata as { movement?: unknown }).movement;
      if (movement === null || typeof movement !== "object") return [];
      const forward = (movement as { forward?: unknown }).forward;
      const direction = (metadata as { direction?: unknown }).direction;
      return typeof forward === "number" &&
        Number.isFinite(forward) &&
        isFiniteVector3(direction)
        ? [{ direction, forward }]
        : [];
    });
  } catch {
    return [];
  }
}

export function readAuthoritativeDisplacements(
  frames: Uint8Array[],
  playerId: string,
): number[] {
  const positions = new Map<
    string,
    { first: [number, number, number]; maximum: number }
  >();
  for (const bytes of frames) {
    try {
      const frame = decodeServerFrame(bytes);
      for (const peer of frame.peers) {
        if (peer.id !== playerId) continue;
        const metadata = peer.metadata;
        if (metadata === null || typeof metadata !== "object") continue;
        const position = (metadata as { position?: unknown }).position;
        if (!isFiniteVector3(position)) continue;
        const tracked = positions.get(peer.id);
        if (tracked === undefined) {
          positions.set(peer.id, { first: position, maximum: 0 });
        } else {
          tracked.maximum = Math.max(
            tracked.maximum,
            Math.hypot(
              position[0] - tracked.first[0],
              position[2] - tracked.first[2],
            ),
          );
        }
      }
    } catch {
      // 非 PEER 帧和关闭竞态不属于移动证据。
    }
  }
  return [...positions.values()].map(({ maximum }) => maximum);
}

function isFiniteVector3(value: unknown): value is [number, number, number] {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    value.every((item) => typeof item === "number" && Number.isFinite(item))
  );
}
