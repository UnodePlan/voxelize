export interface LocalFaceTemplate {
  corners: ReadonlyArray<
    readonly [readonly [number, number, number], readonly [number, number]]
  >;
  dir: readonly [number, number, number];
  name: string;
}

// 顺序和顶点绕序与 Voxelize 服务端 SixFacesBuilder 保持一致。
export const LOCAL_FACE_TEMPLATES: ReadonlyArray<LocalFaceTemplate> = [
  {
    name: "px",
    dir: [1, 0, 0],
    corners: [
      [
        [1, 1, 1],
        [0, 1],
      ],
      [
        [1, 0, 1],
        [0, 0],
      ],
      [
        [1, 1, 0],
        [1, 1],
      ],
      [
        [1, 0, 0],
        [1, 0],
      ],
    ],
  },
  {
    name: "py",
    dir: [0, 1, 0],
    corners: [
      [
        [0, 1, 1],
        [1, 1],
      ],
      [
        [1, 1, 1],
        [0, 1],
      ],
      [
        [0, 1, 0],
        [1, 0],
      ],
      [
        [1, 1, 0],
        [0, 0],
      ],
    ],
  },
  {
    name: "pz",
    dir: [0, 0, 1],
    corners: [
      [
        [0, 0, 1],
        [0, 0],
      ],
      [
        [1, 0, 1],
        [1, 0],
      ],
      [
        [0, 1, 1],
        [0, 1],
      ],
      [
        [1, 1, 1],
        [1, 1],
      ],
    ],
  },
  {
    name: "nx",
    dir: [-1, 0, 0],
    corners: [
      [
        [0, 1, 0],
        [0, 1],
      ],
      [
        [0, 0, 0],
        [0, 0],
      ],
      [
        [0, 1, 1],
        [1, 1],
      ],
      [
        [0, 0, 1],
        [1, 0],
      ],
    ],
  },
  {
    name: "ny",
    dir: [0, -1, 0],
    corners: [
      [
        [1, 0, 1],
        [1, 0],
      ],
      [
        [0, 0, 1],
        [0, 0],
      ],
      [
        [1, 0, 0],
        [1, 1],
      ],
      [
        [0, 0, 0],
        [0, 1],
      ],
    ],
  },
  {
    name: "nz",
    dir: [0, 0, -1],
    corners: [
      [
        [1, 0, 0],
        [0, 0],
      ],
      [
        [0, 0, 0],
        [1, 0],
      ],
      [
        [1, 1, 0],
        [0, 1],
      ],
      [
        [0, 1, 0],
        [1, 1],
      ],
    ],
  },
];
