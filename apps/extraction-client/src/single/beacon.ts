/**
 * 撤离信标：地面外环 + 通天外圈光壁（空心圆柱）。
 * 光壁自下而上渐隐至透明，不画内芯。
 */

import {
  Color,
  CylinderGeometry,
  DoubleSide,
  Group,
  Mesh,
  MeshBasicMaterial,
  RingGeometry,
  ShaderMaterial,
} from "three";

import type { LocalExtractionZone } from "./map";

/** 光壁高度（格）；顶到高空 */
const BEAM_HEIGHT = 110;
/** 光壁相对判定半径的外扩 */
const FOG_RADIUS_SCALE = 1.08;

export function createExtractionBeacon(zone: LocalExtractionZone): Group {
  const group = new Group();
  group.name = "single-extraction-beacon";
  group.position.set(...zone.center);

  const R = Math.max(1.5, zone.radius);
  const fogR = R * FOG_RADIUS_SCALE;

  // 地面外环：对齐可站立判定圈（不需要高度渐隐）
  const outerRing = new Mesh(
    new RingGeometry(R * 0.92, R * 1.08, 64),
    new MeshBasicMaterial({
      color: "#72e6ba",
      depthWrite: false,
      opacity: 0.72,
      transparent: true,
      toneMapped: false,
    }),
  );
  outerRing.rotation.x = -Math.PI / 2;
  outerRing.position.y = 0.05;

  // 通天外圈：高度方向底部实、顶部完全透明
  const fogShell = new Mesh(
    new CylinderGeometry(fogR, fogR * 1.04, BEAM_HEIGHT, 48, 8, true),
    makeHeightFadeMat("#7aefc8", 0.28, BEAM_HEIGHT),
  );
  fogShell.position.y = BEAM_HEIGHT * 0.5;
  fogShell.renderOrder = 1;

  // 近地略加浓一圈外缘（短，同样向上淡一点）
  const baseH = 3.2;
  const baseShell = new Mesh(
    new CylinderGeometry(fogR * 0.98, fogR * 1.1, baseH, 40, 2, true),
    makeHeightFadeMat("#6ee0b8", 0.22, baseH),
  );
  baseShell.position.y = baseH * 0.5;
  baseShell.renderOrder = 2;

  group.add(outerRing, fogShell, baseShell);
  return group;
}

/**
 * 空心圆柱高度渐隐材质。
 * 局部 y 从 -h/2（底）到 +h/2（顶）；顶部 alpha→0。
 */
function makeHeightFadeMat(
  color: string,
  baseOpacity: number,
  height: number,
): ShaderMaterial {
  return new ShaderMaterial({
    uniforms: {
      uColor: { value: new Color(color) },
      uOpacity: { value: baseOpacity },
      uHeight: { value: height },
    },
    transparent: true,
    depthWrite: false,
    side: DoubleSide,
    toneMapped: false,
    vertexShader: /* glsl */ `
      uniform float uHeight;
      varying float vT;

      void main() {
        // 0 = 底部，1 = 顶部
        vT = clamp((position.y + uHeight * 0.5) / max(uHeight, 1e-4), 0.0, 1.0);
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }
    `,
    fragmentShader: /* glsl */ `
      uniform vec3 uColor;
      uniform float uOpacity;
      varying float vT;

      void main() {
        // 下部保持可见，中上部缓慢淡出，顶端透明
        // pow > 1：上半段淡得更快，底部仍清晰
        float fade = pow(1.0 - vT, 1.45);
        float alpha = uOpacity * fade;
        if (alpha < 0.003) discard;
        gl_FragColor = vec4(uColor, alpha);
      }
    `,
  });
}
