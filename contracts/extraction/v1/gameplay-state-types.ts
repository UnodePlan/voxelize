import type { DeathResultEnvelope, HealthStateEnvelope } from "./combat-types";
import type { ExtractionStateEnvelope } from "./extraction-types";
import type { MiningStateEnvelope, ResourceKey } from "./types";

export interface DecodedGetStateIntent {
  requestId: string;
  sequence: number;
  payload: Record<string, never>;
}

export interface ResourceStackState {
  resource: ResourceKey;
  quantity: number;
}

export interface InventoryState {
  slots: Array<ResourceStackState | null>;
  revision: number;
  frozen: boolean;
}

export interface FixedEquipmentState {
  pickaxe: "basic_pickaxe";
  meleeWeapon: "basic_melee_weapon";
}

export interface AttackCursorState {
  revision: number;
  acceptedSequence: number | null;
}

export interface GameplayStateData {
  matchId: string;
  inventory: InventoryState;
  equipment: FixedEquipmentState;
  mining: MiningStateEnvelope;
  extraction: ExtractionStateEnvelope;
  health: HealthStateEnvelope;
  attack: AttackCursorState;
  deathResult: DeathResultEnvelope | null;
}
