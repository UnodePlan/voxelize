import { Entity, type World } from "@voxelize/core";
import { Group } from "three";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { disposeObjectTree } from "./object-disposal";

type LootContents = { diamond?: number; dirt?: number; gold?: number };

interface LootMetadata {
  loot?: {
    contents?: LootContents;
    revision?: number;
  };
  position?: [number, number, number];
}

export function createLootEntityClass(
  world: World,
  manifest: ExtractionManifest,
): new (id: string) => Entity<LootMetadata> {
  return class ExtractionLootEntity extends Entity<LootMetadata> {
    private visual: Group | null = null;
    private revision = -1;

    onCreate = (metadata: LootMetadata): void => this.apply(metadata);

    onUpdate = (metadata: LootMetadata): void => this.apply(metadata);

    onDelete = (): void => {
      this.disposeVisual();
    };

    update = (): void => {
      if (this.visual !== null) this.visual.rotation.y += 0.012;
    };

    private apply(metadata: LootMetadata): void {
      const position = metadata.position;
      if (validVector(position)) this.position.set(...position);
      const revision = metadata.loot?.revision;
      if (
        typeof revision !== "number" ||
        !Number.isSafeInteger(revision) ||
        revision === this.revision
      ) {
        return;
      }
      this.revision = revision;
      this.disposeVisual();
      this.visual = this.makeVisual(metadata.loot?.contents);
      if (this.visual !== null) this.add(this.visual);
    }

    private makeVisual(contents: LootContents | undefined): Group | null {
      const key =
        (contents?.diamond ?? 0) > 0
          ? "diamond"
          : (contents?.gold ?? 0) > 0
            ? "gold"
            : "dirt";
      const definition = manifest.resources.find(
        (resource) => resource.key === key,
      );
      if (definition === undefined) return null;
      const visual = world.makeBlockMesh(definition.voxelId, {
        material: "standard",
      });
      visual.scale.setScalar(0.42);
      visual.position.y = 0.3;
      return visual;
    }

    private disposeVisual(): void {
      if (this.visual === null) return;
      // 方块材质引用 World 图集；这里只释放实例几何和材质本身。
      disposeObjectTree(this.visual, false);
      this.visual = null;
    }
  };
}

function validVector(
  value: [number, number, number] | undefined,
): value is [number, number, number] {
  return (
    Array.isArray(value) &&
    value.length === 3 &&
    value.every((part) => Number.isFinite(part))
  );
}
