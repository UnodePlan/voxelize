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
  lastDropSequence: number | null;
}

export interface FixedEquipmentState {
  pickaxe: "basic_pickaxe";
  meleeWeapon: "basic_melee_weapon";
}

export interface InventoryStateData {
  inventory: InventoryState;
  equipment: FixedEquipmentState;
}

export interface InventoryStateEnvelope {
  protocolVersion: number;
  type: "state";
  matchId: string;
  stream: "inventory";
  revision: number;
  data: InventoryStateData;
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
