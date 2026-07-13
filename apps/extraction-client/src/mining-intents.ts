import type {
  IntentEnvelope,
  MiningPayload,
} from "../../../contracts/extraction/v1/typescript";

const MAX_U32 = 4_294_967_295;
const MIN_I32 = -2_147_483_648;
const MAX_I32 = 2_147_483_647;

export class MiningIntentFactory {
  private nextSequence: number;

  constructor(
    private readonly protocolVersion: number,
    lastAcceptedSequence: number | null = null,
  ) {
    if (
      !Number.isInteger(protocolVersion) ||
      protocolVersion < 0 ||
      protocolVersion > MAX_U32
    ) {
      throw new Error("protocolVersion 必须是 u32");
    }
    if (
      lastAcceptedSequence !== null &&
      (!Number.isInteger(lastAcceptedSequence) ||
        lastAcceptedSequence < 0 ||
        lastAcceptedSequence > MAX_U32)
    ) {
      throw new Error("lastAcceptedSequence 必须是 u32 或 null");
    }
    this.nextSequence =
      lastAcceptedSequence === null ? 0 : lastAcceptedSequence + 1;
  }

  start(voxel: readonly [number, number, number]): IntentEnvelope {
    if (
      voxel.some(
        (coordinate) =>
          !Number.isInteger(coordinate) ||
          coordinate < MIN_I32 ||
          coordinate > MAX_I32,
      )
    ) {
      throw new Error("挖掘目标必须是 i32 三元坐标");
    }
    return this.build({ action: "start", voxel: [...voxel] });
  }

  maintain(): IntentEnvelope {
    return this.build({ action: "maintain" });
  }

  cancel(): IntentEnvelope {
    return this.build({ action: "cancel" });
  }

  private build(payload: MiningPayload): IntentEnvelope {
    if (this.nextSequence > MAX_U32) {
      throw new Error("挖掘 sequence 已耗尽，必须重新同步比赛状态");
    }
    const envelope: IntentEnvelope = {
      protocolVersion: this.protocolVersion,
      type: "intent",
      requestId: crypto.randomUUID(),
      sequence: this.nextSequence,
      payload,
    };
    this.nextSequence += 1;
    return envelope;
  }
}
