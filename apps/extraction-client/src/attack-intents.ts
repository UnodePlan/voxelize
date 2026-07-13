import type {
  AttackPayload,
  IntentEnvelope,
} from "../../../contracts/extraction/v1/typescript";

const MAX_U32 = 4_294_967_295;

export class AttackIntentFactory {
  private nextSequence: number;

  constructor(
    private readonly protocolVersion: number,
    lastAcceptedSequence: number | null = null,
  ) {
    if (!isU32(protocolVersion)) {
      throw new Error("protocolVersion 必须是 u32");
    }
    if (lastAcceptedSequence !== null && !isU32(lastAcceptedSequence)) {
      throw new Error("lastAcceptedSequence 必须是 u32 或 null");
    }
    this.nextSequence =
      lastAcceptedSequence === null ? 0 : lastAcceptedSequence + 1;
  }

  attack(): IntentEnvelope {
    return this.build({ weaponSlot: "melee" });
  }

  private build(payload: AttackPayload): IntentEnvelope {
    if (this.nextSequence > MAX_U32) {
      throw new Error("攻击 sequence 已耗尽，必须重新同步比赛状态");
    }
    const envelope: IntentEnvelope = {
      protocolVersion: this.protocolVersion,
      type: "intent",
      requestId: crypto.randomUUID(),
      sequence: this.nextSequence,
      payload: { weaponSlot: payload.weaponSlot },
    };
    this.nextSequence += 1;
    return envelope;
  }
}

function isU32(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= MAX_U32;
}
