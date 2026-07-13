import { readRecord, readVector3, type LiveVector3 } from "./gameplay-state";
import type { LiveEntity, LiveMethod, LiveServerFrame } from "./wire";

const RESULT_METHOD = "pvp:v1:result";

export class GameplayFrameStore {
  readonly playerId: string;
  private readonly directions = new Map<string, LiveVector3>();
  private readonly entities = new Map<string, LiveEntity>();
  private readonly lootCreateCounts = new Map<string, number>();
  private readonly methodStates = new Map<string, unknown>();
  private readonly observedLootIds = new Set<string>();
  private readonly peers = new Map<string, LiveVector3>();
  private readonly peerRevisions = new Map<string, number>();
  private readonly removedLootIds = new Set<string>();

  constructor(init: LiveServerFrame) {
    if (init.type !== "INIT")
      throw new Error("gameplay frame store requires INIT");
    const source = readRecord(init.json, "INIT json");
    if (typeof source.id !== "string" || source.id === "") {
      throw new Error("INIT json omitted the public player ID");
    }
    this.playerId = source.id;
    if (source.savedPosition !== undefined) {
      this.peers.set(
        this.playerId,
        readVector3(source.savedPosition, "INIT savedPosition"),
      );
    }
    if (source.savedDirection !== undefined) {
      this.directions.set(
        this.playerId,
        readVector3(source.savedDirection, "INIT savedDirection"),
      );
    }
    this.apply(init);
  }

  apply(frame: LiveServerFrame): LiveMethod | null {
    for (const peer of frame.peers) {
      const metadata = readRecord(peer.metadata, "peer metadata");
      if (metadata.position !== undefined || metadata.direction !== undefined) {
        this.peerRevisions.set(
          peer.id,
          (this.peerRevisions.get(peer.id) ?? 0) + 1,
        );
      }
      if (metadata.position !== undefined) {
        this.peers.set(
          peer.id,
          readVector3(metadata.position, "peer position"),
        );
      }
      if (metadata.direction !== undefined) {
        this.directions.set(
          peer.id,
          readVector3(metadata.direction, "peer direction"),
        );
      }
    }
    for (const entity of frame.entities) {
      if (entity.operation === "DELETE") {
        if (this.entities.get(entity.id)?.type === "extraction:loot") {
          this.removedLootIds.add(entity.id);
        }
        this.entities.delete(entity.id);
      } else {
        this.entities.set(entity.id, entity);
        if (entity.type === "extraction:loot") {
          this.observedLootIds.add(entity.id);
          if (entity.operation === "CREATE") {
            this.lootCreateCounts.set(
              entity.id,
              (this.lootCreateCounts.get(entity.id) ?? 0) + 1,
            );
          }
        }
      }
    }
    const method = frame.type === "METHOD" ? frame.method : null;
    if (method !== null && method.name !== RESULT_METHOD) {
      this.methodStates.set(method.name, method.payload);
    }
    return method;
  }

  position(playerId = this.playerId): LiveVector3 | null {
    const position = this.peers.get(playerId);
    return position === undefined ? null : [...position];
  }

  direction(playerId = this.playerId): LiveVector3 | null {
    const direction = this.directions.get(playerId);
    return direction === undefined ? null : [...direction];
  }

  peerRevision(playerId = this.playerId): number {
    return this.peerRevisions.get(playerId) ?? 0;
  }

  latestMethodState(name: string): unknown {
    return this.methodStates.get(name);
  }

  hasMethodState(name: string): boolean {
    return this.methodStates.has(name);
  }

  visibleLoot(): LiveEntity[] {
    return [...this.entities.values()].filter(
      (entity) => entity.type === "extraction:loot",
    );
  }

  observedLoot(): string[] {
    return [...this.observedLootIds].sort();
  }

  lootCreationCount(id: string): number {
    return this.lootCreateCounts.get(id) ?? 0;
  }

  removedLoot(): string[] {
    return [...this.removedLootIds].sort();
  }
}
