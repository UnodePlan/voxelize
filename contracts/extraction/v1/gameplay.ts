import { assertOnlyKeys, readUnsignedInteger } from "./decoder-utils";
import type {
  DecodedDropSlotIntent,
  DropSlotPayload,
  ProtocolEnvelope,
} from "./types";

const RESOURCE_BACKPACK_SLOTS = 12;

export function decodeDropSlotIntent(
  envelope: ProtocolEnvelope,
): DecodedDropSlotIntent {
  if (envelope.type !== "intent") {
    throw new Error("drop-slot: expected intent envelope");
  }
  const payload = envelope.payload;
  assertOnlyKeys(
    payload,
    ["slot", "expectedInventoryRevision"],
    "drop-slot.payload",
  );
  const decoded: DropSlotPayload = {
    slot: readUnsignedInteger(payload.slot, "drop-slot.payload.slot"),
    expectedInventoryRevision: readUnsignedInteger(
      payload.expectedInventoryRevision,
      "drop-slot.payload.expectedInventoryRevision",
    ),
  };
  if (decoded.slot >= RESOURCE_BACKPACK_SLOTS) {
    throw new Error("drop-slot.payload.slot: out of range");
  }
  return {
    requestId: envelope.requestId,
    sequence: envelope.sequence,
    payload: decoded,
  };
}
