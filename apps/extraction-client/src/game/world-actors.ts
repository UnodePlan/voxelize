import {
  Character,
  Entities,
  Peers,
  type RigidControls,
  type World,
} from "@voxelize/core";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { createLootEntityClass } from "./loot-entity";

export interface PeerMetadata {
  direction?: [number, number, number];
  position?: [number, number, number];
}

export interface WorldActors {
  entities: Entities;
  peers: Peers<Character, PeerMetadata>;
}

export function createWorldActors(
  world: World,
  controls: RigidControls,
  manifest: ExtractionManifest,
): WorldActors {
  const peers = new Peers<Character, PeerMetadata>(controls.object);
  peers.createPeer = () => new Character();
  peers.onPeerUpdate = (character, metadata) => {
    if (
      character !== undefined &&
      validVector(metadata?.position) &&
      validVector(metadata?.direction)
    ) {
      character.set(metadata.position, metadata.direction);
    }
  };
  peers.packInfo = () => undefined;
  world.add(peers);

  const entities = new Entities();
  entities.setClass("extraction:loot", createLootEntityClass(world, manifest));
  world.add(entities);
  return { entities, peers };
}

function validVector(value: unknown): value is [number, number, number] {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    value.every((part) => typeof part === "number" && Number.isFinite(part))
  );
}
