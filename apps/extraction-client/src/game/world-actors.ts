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

/** 可读局内昵称：优先服务端 username，否则截断 id */
export function peerDisplayName(id: string, username?: string): string {
  const label = username?.trim();
  if (label !== undefined && label.length > 0) return label.slice(0, 24);
  if (id.length <= 8) return id || "Player";
  return `P-${id.slice(0, 6)}`;
}

/** 稳定配色，便于多人区分 */
export function peerPalette(id: string): {
  body: string;
  head: string;
  arms: string;
  legs: string;
} {
  let hash = 0;
  for (let i = 0; i < id.length; i += 1) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  const hue = hash % 360;
  const body = `hsl(${hue} 42% 32%)`;
  const head = `hsl(${(hue + 28) % 360} 48% 62%)`;
  const arms = `hsl(${(hue + 12) % 360} 38% 48%)`;
  const legs = `hsl(${(hue + 200) % 360} 30% 40%)`;
  return { body, head, arms, legs };
}

export function createWorldActors(
  world: World,
  controls: RigidControls,
  manifest: ExtractionManifest,
): WorldActors {
  const peers = new Peers<Character, PeerMetadata>(controls.object);
  peers.createPeer = (id: string) => {
    const palette = peerPalette(id);
    const character = new Character({
      nameTagOptions: {
        fontFace: "system-ui, sans-serif",
        fontSize: 0.18,
        yOffset: 0.35,
        backgroundColor: "rgba(0,0,0,0.55)",
      },
      body: { color: palette.body },
      head: { color: palette.head, faceColor: "#f2d2c0" },
      arms: { color: palette.arms },
      legs: { color: palette.legs },
    });
    character.username = peerDisplayName(id);
    character.visible = true;
    return character;
  };
  peers.onPeerUpdate = (character, metadata, info) => {
    if (character === undefined) return;
    if (info?.username !== undefined && info.username.trim() !== "") {
      character.username = peerDisplayName(info.id, info.username);
    }
    if (validVector(metadata?.position) && validVector(metadata?.direction)) {
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
