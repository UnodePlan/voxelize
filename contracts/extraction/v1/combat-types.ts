export const ATTACK_WEAPON_SLOTS = ["melee"] as const;
export const ATTACK_RESOLUTIONS = ["miss", "hit", "kill"] as const;
export const DEATH_CAUSES = ["melee", "reconnectTimeout"] as const;

export type AttackWeaponSlot = (typeof ATTACK_WEAPON_SLOTS)[number];
export type AttackResolution = (typeof ATTACK_RESOLUTIONS)[number];
export type DeathCause = (typeof DEATH_CAUSES)[number];

export interface AttackPayload {
  weaponSlot: AttackWeaponSlot;
}

export interface DecodedAttackIntent {
  requestId: string;
  sequence: number;
  payload: AttackPayload;
}

export interface AttackResultData {
  acceptedSequence: number;
  attackRevision: number;
  resolution: AttackResolution;
}

export type HealthStateData =
  | {
      status: "alive";
      currentHalfHearts: number;
      maxHalfHearts: 20;
    }
  | {
      status: "dead";
      currentHalfHearts: 0;
      maxHalfHearts: 20;
    };

export interface HealthStateEnvelope {
  protocolVersion: number;
  type: "state";
  matchId: string;
  stream: "health";
  revision: number;
  data: HealthStateData;
}

export interface ResourceTally {
  dirt: number;
  gold: number;
  diamond: number;
}

export interface DeathResultData {
  cause: DeathCause;
  killerPublicPlayerId: string | null;
  survivedMs: number;
  mined: ResourceTally;
  pickedUp: ResourceTally;
  lost: ResourceTally;
}

export interface DeathResultEnvelope {
  protocolVersion: number;
  type: "state";
  matchId: string;
  stream: "deathResult";
  revision: number;
  data: DeathResultData;
}
