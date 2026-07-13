import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

export interface GameNetworkEvents {
  onAuthenticationInvalidated(): void;
  onConnection(state: "offline" | "online" | "reconnecting"): void;
  onGameplayState(state: GameplayStateData): void;
  onProtocolError(message: string): void;
  onReconnectExpired(): void;
}

export interface PendingRequest {
  reject(error: Error): void;
  resolve(state: GameplayStateData): void;
  timeout: ReturnType<typeof setTimeout>;
}
