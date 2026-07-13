export const RESOURCE_KEYS = ["dirt", "gold", "diamond"] as const;
export const EQUIPMENT_KEYS = ["basic_pickaxe", "basic_melee_weapon"] as const;
export const ERROR_CODES = [
  "PROTOCOL_UNSUPPORTED_VERSION",
  "REQUEST_MALFORMED",
  "AUTH_REQUIRED",
  "AUTH_WRONG_NETWORK",
  "AUTH_INVALID_SIWE",
  "AUTH_NONCE_INVALID",
  "MATCH_FULL",
  "MATCH_ROSTER_LOCKED",
  "MATCH_RECONNECT_EXPIRED",
  "GAME_INVALID_STATE",
  "GAME_STALE_SEQUENCE",
  "GAME_STALE_REVISION",
  "GAME_OUT_OF_RANGE",
  "GAME_COOLDOWN",
  "INVENTORY_SLOT_INVALID",
  "SETTLEMENT_PENDING",
  "SETTLEMENT_CONFLICT",
  "SERVICE_UNAVAILABLE",
] as const;

export type JsonRecord = Record<string, unknown>;
export type ResourceKey = (typeof RESOURCE_KEYS)[number];
export type EquipmentKey = (typeof EQUIPMENT_KEYS)[number];
export type ErrorCode = (typeof ERROR_CODES)[number];

export interface ResourceDefinition {
  key: ResourceKey;
  voxelId: number;
  itemId: number;
  maxStack: number;
  scoreWeight: number;
}

export interface EquipmentDefinition {
  key: EquipmentKey;
  itemId: number;
}

export interface ExtractionManifest {
  protocolVersion: number;
  catalogVersion: number;
  gameplayVersion: string;
  generationVersion: string;
  configVersion: string;
  resources: ResourceDefinition[];
  equipment: EquipmentDefinition[];
  errorCodes: ErrorCode[];
}

export type ProtocolEnvelope = IntentEnvelope | ResultEnvelope;

export interface IntentEnvelope {
  protocolVersion: number;
  type: "intent";
  requestId: string;
  sequence: number;
  payload: JsonRecord;
}

export interface ResultEnvelope {
  protocolVersion: number;
  type: "result";
  requestId: string;
  outcome:
    | { status: "ok"; data: unknown }
    | {
        status: "error";
        error: { code: ErrorCode; retryable: boolean };
      };
}

export interface DropSlotPayload {
  slot: number;
  expectedInventoryRevision: number;
}

export interface DecodedDropSlotIntent {
  requestId: string;
  sequence: number;
  payload: DropSlotPayload;
}

export interface EnvelopeFixtureCase {
  name: string;
  route: string;
  accept: boolean;
  value: unknown;
}

export interface EnvelopeFixture {
  fixtureVersion: number;
  cases: EnvelopeFixtureCase[];
}
