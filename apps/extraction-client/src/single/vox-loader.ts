/**
 * 最小 MagicaVoxel .vox 解析（VOX 150+：SIZE / XYZI / RGBA）。
 * 输出可直接喂 three BufferGeometry 的体素列表。
 */

export interface VoxModel {
  size: { x: number; y: number; z: number };
  /** 体素坐标为 MagicaVoxel 空间（Z 向上） */
  voxels: Array<{ x: number; y: number; z: number; colorIndex: number }>;
  /** 调色板 index 1–255 → RGBA 0–255；0 未用 */
  palette: Array<[number, number, number, number]>;
}

const DEFAULT_PALETTE: Array<[number, number, number, number]> = (() => {
  const p: Array<[number, number, number, number]> = Array.from(
    { length: 256 },
    () => [200, 200, 200, 255],
  );
  p[0] = [0, 0, 0, 0];
  return p;
})();

export function parseVox(buffer: ArrayBuffer): VoxModel {
  const view = new DataView(buffer);
  const bytes = new Uint8Array(buffer);
  if (
    bytes[0] !== 0x56 ||
    bytes[1] !== 0x4f ||
    bytes[2] !== 0x58 ||
    bytes[3] !== 0x20
  ) {
    throw new Error("不是有效的 VOX 文件");
  }

  let size = { x: 1, y: 1, z: 1 };
  let voxels: VoxModel["voxels"] = [];
  let palette = DEFAULT_PALETTE.map(
    (c) => [...c] as [number, number, number, number],
  );

  const readChunks = (start: number, end: number): void => {
    let offset = start;
    while (offset + 12 <= end) {
      const id = String.fromCharCode(
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
      );
      const contentSize = view.getUint32(offset + 4, true);
      const childrenSize = view.getUint32(offset + 8, true);
      const contentStart = offset + 12;
      const contentEnd = contentStart + contentSize;
      if (contentEnd + childrenSize > end + 1e-9 && contentEnd > end) break;

      if (id === "SIZE" && contentSize >= 12) {
        size = {
          x: view.getInt32(contentStart, true),
          y: view.getInt32(contentStart + 4, true),
          z: view.getInt32(contentStart + 8, true),
        };
      } else if (id === "XYZI" && contentSize >= 4) {
        const n = view.getUint32(contentStart, true);
        voxels = [];
        let p = contentStart + 4;
        for (let i = 0; i < n && p + 4 <= contentEnd; i += 1) {
          voxels.push({
            x: bytes[p],
            y: bytes[p + 1],
            z: bytes[p + 2],
            colorIndex: bytes[p + 3],
          });
          p += 4;
        }
      } else if (id === "RGBA" && contentSize >= 1024) {
        palette = Array.from({ length: 256 }, () => [0, 0, 0, 0] as [
          number,
          number,
          number,
          number,
        ]);
        for (let i = 0; i < 255; i += 1) {
          const o = contentStart + i * 4;
          palette[i + 1] = [
            bytes[o],
            bytes[o + 1],
            bytes[o + 2],
            bytes[o + 3],
          ];
        }
      }

      if (childrenSize > 0) {
        readChunks(contentEnd, contentEnd + childrenSize);
      }
      offset = contentEnd + childrenSize;
    }
  };

  // MAIN 从 offset 8 开始
  readChunks(8, buffer.byteLength);

  return { size, voxels, palette };
}

export async function loadVoxUrl(url: string): Promise<VoxModel> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`加载 VOX 失败: ${url} (${res.status})`);
  return parseVox(await res.arrayBuffer());
}
