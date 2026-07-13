import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

const MAX_U32 = 4_294_967_295;

export class IntentSequence {
  private value = 0;

  next(): number {
    if (this.value >= MAX_U32) throw new Error("Intent sequence exhausted");
    this.value += 1;
    return this.value;
  }

  seed(state: GameplayStateData): void {
    this.value = Math.max(
      this.value,
      state.attack.acceptedSequence ?? 0,
      state.mining.data.acceptedSequence ?? 0,
      state.inventory.lastDropSequence ?? 0,
    );
  }
}
