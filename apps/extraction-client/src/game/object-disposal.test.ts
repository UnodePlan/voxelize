import { BoxGeometry, Group, Mesh, MeshBasicMaterial, Texture } from "three";
import { describe, expect, it, vi } from "vitest";

import { disposeObjectTree } from "./object-disposal";

describe("disposeObjectTree", () => {
  it("releases geometry and material while preserving a shared atlas", () => {
    const root = new Group();
    const texture = new Texture();
    const geometry = new BoxGeometry();
    const material = new MeshBasicMaterial({ map: texture });
    const disposeGeometry = vi.spyOn(geometry, "dispose");
    const disposeMaterial = vi.spyOn(material, "dispose");
    const disposeTexture = vi.spyOn(texture, "dispose");
    root.add(new Mesh(geometry, material));

    disposeObjectTree(root, false);

    expect(disposeGeometry).toHaveBeenCalledOnce();
    expect(disposeMaterial).toHaveBeenCalledOnce();
    expect(disposeTexture).not.toHaveBeenCalled();
    expect(root.children).toHaveLength(0);
  });

  it("releases owned textures for peer and interaction trees", () => {
    const root = new Group();
    const texture = new Texture();
    const material = new MeshBasicMaterial({ map: texture });
    const disposeTexture = vi.spyOn(texture, "dispose");
    root.add(new Mesh(new BoxGeometry(), material));

    disposeObjectTree(root, true);

    expect(disposeTexture).toHaveBeenCalledOnce();
  });
});
