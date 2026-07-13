import type { World } from "@voxelize/core";
import { Color } from "three";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

const ALL_FACES = ["px", "nx", "py", "ny", "pz", "nz"];
const RESOURCE_COLORS = {
  diamond: new Color("#43d8cf"),
  dirt: new Color("#756a55"),
  gold: new Color("#e2b93f"),
} as const;

export async function applyExtractionTextures(
  world: World,
  manifest: ExtractionManifest,
): Promise<void> {
  await world.applyBlockTextures(
    manifest.resources.map((resource) => ({
      idOrName: resource.voxelId,
      faceNames: ALL_FACES,
      source: RESOURCE_COLORS[resource.key],
    })),
  );
}
