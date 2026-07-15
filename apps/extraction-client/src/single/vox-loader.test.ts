import { describe, expect, it } from "vitest";

import { parseVox } from "./vox-loader";

/** 构造最小合法 VOX：1 体素在 (1,2,3) color=1，SIZE 4x4x4，默认无 RGBA 用灰 */
function buildTinyVox(): ArrayBuffer {
  const parts: number[] = [];
  const u8 = (n: number) => parts.push(n & 255);
  const u32 = (n: number) => {
    u8(n);
    u8(n >> 8);
    u8(n >> 16);
    u8(n >> 24);
  };
  const i32 = u32;
  const fourcc = (s: string) => {
    for (const c of s) u8(c.charCodeAt(0));
  };

  // VOX  + version 150
  fourcc("VOX ");
  u32(150);
  // MAIN content=0 children= SIZE(12+12) + XYZI(4+4+4)
  const sizeContent = 12;
  const xyziContent = 4 + 4; // n + one voxel
  const childrenSize = 12 + sizeContent + 12 + xyziContent;
  fourcc("MAIN");
  u32(0);
  u32(childrenSize);
  fourcc("SIZE");
  u32(sizeContent);
  u32(0);
  i32(4);
  i32(4);
  i32(4);
  fourcc("XYZI");
  u32(xyziContent);
  u32(0);
  u32(1); // 1 voxel
  u8(1);
  u8(2);
  u8(3);
  u8(1); // color index 1

  return new Uint8Array(parts).buffer;
}

describe("parseVox", () => {
  it("reads SIZE and XYZI", () => {
    const model = parseVox(buildTinyVox());
    expect(model.size).toEqual({ x: 4, y: 4, z: 4 });
    expect(model.voxels).toHaveLength(1);
    expect(model.voxels[0]).toMatchObject({
      x: 1,
      y: 2,
      z: 3,
      colorIndex: 1,
    });
  });
});
