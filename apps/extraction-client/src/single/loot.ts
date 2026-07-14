import {
  BoxGeometry,
  Group,
  Mesh,
  MeshBasicMaterial,
  Vector3,
  type Object3D,
} from "three";

import { disposeObjectTree } from "../game/object-disposal";

import type { LocalResourceKey } from "./state";

interface LocalLootEntry {
  availableAt: number;
  baseY: number;
  mesh: Mesh;
  quantity: number;
  resource: LocalResourceKey;
}

const RESOURCE_COLORS: Record<LocalResourceKey, string> = {
  dirt: "#79523d",
  gold: "#e0b43b",
  diamond: "#3dd2cc",
};

export class LocalLootSystem {
  private readonly root = new Group();
  private readonly entries: LocalLootEntry[] = [];

  constructor(world: Object3D) {
    this.root.name = "single-local-loot";
    world.add(this.root);
  }

  drop(
    resource: LocalResourceKey,
    quantity: number,
    position: readonly [number, number, number],
    now: number,
    pickupDelayMs: number,
  ): void {
    if (quantity <= 0) return;
    const mesh = new Mesh(
      new BoxGeometry(0.42, 0.42, 0.42),
      new MeshBasicMaterial({ color: RESOURCE_COLORS[resource] }),
    );
    mesh.position.set(position[0], position[1], position[2]);
    mesh.rotation.set(0.18, 0.34, 0.08);
    mesh.userData.resource = resource;
    this.root.add(mesh);
    this.entries.push({
      availableAt: now + pickupDelayMs,
      baseY: position[1],
      mesh,
      quantity,
      resource,
    });
  }

  updateAndCollect(
    player: Vector3,
    now: number,
    accept: (resource: LocalResourceKey, quantity: number) => number,
  ): void {
    for (let index = this.entries.length - 1; index >= 0; index -= 1) {
      const entry = this.entries[index];
      entry.mesh.rotation.y += 0.018;
      entry.mesh.position.y =
        entry.baseY + Math.sin(now * 0.004 + index) * 0.07;
      if (
        now < entry.availableAt ||
        entry.mesh.position.distanceToSquared(player) > 2.25
      ) {
        continue;
      }
      const remainder = accept(entry.resource, entry.quantity);
      if (remainder > 0) {
        entry.quantity = remainder;
        entry.availableAt = now + 350;
        continue;
      }
      this.entries.splice(index, 1);
      disposeObjectTree(entry.mesh, true);
    }
  }

  dispose(): void {
    this.entries.length = 0;
    disposeObjectTree(this.root, true);
  }
}
