import type { MessageProtocol } from "@voxelize/protocol";

import type {
  AttackResultData,
  GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

export interface GameNetworkEvents {
  onAuthenticationInvalidated(): void;
  onConnection(state: "offline" | "online" | "reconnecting"): void;
  onGameplayState(state: GameplayStateData): void;
  /** 近战攻击 METHOD 结果（hit/kill/miss），与完整 gameplay state 分路 */
  onAttackResult?(result: AttackResultData): void;
  onProtocolError(message: string): void;
  onReconnectExpired(): void;
  onVoxelMessage?(message: MessageProtocol): void;
  onVoxelReset?(): void;
}

export interface PendingRequest {
  reject(error: Error): void;
  resolve(state: GameplayStateData): void;
  timeout: ReturnType<typeof setTimeout>;
}
