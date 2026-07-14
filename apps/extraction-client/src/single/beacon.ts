import {
  BoxGeometry,
  CylinderGeometry,
  Group,
  Mesh,
  MeshBasicMaterial,
  RingGeometry,
} from "three";

import type { LocalExtractionZone } from "./map";

export function createExtractionBeacon(zone: LocalExtractionZone): Group {
  const group = new Group();
  group.name = "single-extraction-beacon";
  group.position.set(...zone.center);

  const ringMaterial = new MeshBasicMaterial({
    color: "#72e6ba",
    depthWrite: false,
    opacity: 0.55,
    transparent: true,
    toneMapped: false,
  });
  const ring = new Mesh(
    new RingGeometry(zone.radius * 0.55, zone.radius * 0.68, 48),
    ringMaterial,
  );
  ring.rotation.x = -Math.PI / 2;
  ring.position.y = 0.04;

  // 短柱体而非通天光柱，减少对天空的视觉污染。
  const beam = new Mesh(
    new CylinderGeometry(0.04, 0.07, 1.6, 8),
    new MeshBasicMaterial({
      color: "#8ef0cb",
      depthWrite: false,
      opacity: 0.28,
      transparent: true,
      toneMapped: false,
    }),
  );
  beam.position.y = 0.95;

  const plinth = new Mesh(
    new BoxGeometry(0.7, 0.14, 0.7),
    new MeshBasicMaterial({
      color: "#4fbf94",
      depthWrite: false,
      opacity: 0.7,
      transparent: true,
      toneMapped: false,
    }),
  );
  plinth.position.y = 0.07;

  group.add(ring, beam, plinth);
  return group;
}
