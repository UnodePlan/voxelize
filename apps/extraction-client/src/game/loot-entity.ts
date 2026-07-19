import { Entity, type World } from "@voxelize/core";
import { Group, type Object3D } from "three";

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

/**
 * 世界掉落：方块缩略 + 上下浮动 + 自转。
 * 多资源时叠放 1–3 个小方块，表达「一坨掉落」。
 */
export function createLootEntityClass(
  world: World,
  manifest: ExtractionManifest,
): new (id: string) => Entity<LootMetadata> {
  return class ExtractionLootEntity extends Entity<LootMetadata> {
    private visual: Group | null = null;
    private revision = -1;
    private bobPhase = Math.random() * Math.PI * 2;
    private baseY = 0.3;

    onCreate = (metadata: LootMetadata): void => this.apply(metadata);

    onUpdate = (metadata: LootMetadata): void => this.apply(metadata);

    onDelete = (): void => {
      this.disposeVisual();
    };

    update = (): void => {
      if (this.visual === null) return;
      this.bobPhase += 0.04;
      this.visual.rotation.y += 0.02;
      this.visual.position.y = this.baseY + Math.sin(this.bobPhase) * 0.08;
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
      const stack = resourceStack(contents);
      if (stack.length === 0) return null;
      const group = new Group();
      group.name = "extraction-loot-visual";
      stack.forEach((key, index) => {
        const definition = manifest.resources.find(
          (resource) => resource.key === key,
        );
        if (definition === undefined) return;
        const mesh = world.makeBlockMesh(definition.voxelId, {
          material: "standard",
        });
        // 主块较大，后续资源略小并抬高，表示堆叠
        const scale = index === 0 ? 0.4 : 0.28;
        mesh.scale.setScalar(scale);
        mesh.position.set(
          index === 0 ? 0 : (index - 1) * 0.12 - 0.06,
          index * 0.22,
          index === 0 ? 0 : 0.04,
        );
        mesh.rotation.y = index * 0.4;
        group.add(mesh as Object3D);
      });
      if (group.children.length === 0) return null;
      this.baseY = 0.28;
      group.position.y = this.baseY;
      return group;
    }

    private disposeVisual(): void {
      if (this.visual === null) return;
      disposeObjectTree(this.visual, false);
      this.visual = null;
    }
  };
}

/** 按稀有度优先展示至多 3 种资源块 */
export function resourceStack(
  contents: LootContents | undefined,
): Array<"dirt" | "gold" | "diamond"> {
  if (contents === undefined) return ["dirt"];
  const keys: Array<"dirt" | "gold" | "diamond"> = [];
  if ((contents.diamond ?? 0) > 0) keys.push("diamond");
  if ((contents.gold ?? 0) > 0) keys.push("gold");
  if ((contents.dirt ?? 0) > 0) keys.push("dirt");
  if (keys.length === 0) return ["dirt"];
  return keys.slice(0, 3);
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
