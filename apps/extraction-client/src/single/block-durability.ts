/**
 * 世界方块耐久（按体素坐标持久化）。
 * - 进度为 0–1 比例，与当前工具无关：换工具只改破坏速度，不重置进度
 * - 松开挖掘 / 换目标 / 他人继续挖，都从上一次 damage 接着算
 * - blockId 变化（方块被替换）时自动清零
 */

export interface BlockDurabilityEntry {
  /** 0–1，1 表示可破坏完成 */
  damage: number;
  /** 记录开始挖时的方块 id，防止错配 */
  blockId: number;
}

export class LocalBlockDurability {
  private readonly byKey = new Map<string, BlockDurabilityEntry>();

  /** 当前损坏比例；无记录或 blockId 不匹配时为 0 */
  get(key: string, blockId: number): number {
    const entry = this.byKey.get(key);
    if (entry === undefined || entry.blockId !== blockId) return 0;
    return entry.damage;
  }

  /**
   * 累加挖掘量。
   * @param amount 本次增加的比例（通常 = deltaMs / requiredMs）
   * @returns 更新后的 0–1 damage
   */
  apply(key: string, blockId: number, amount: number): number {
    if (!(amount > 0) || !Number.isFinite(amount)) {
      return this.get(key, blockId);
    }
    const previous = this.get(key, blockId);
    const damage = Math.min(1, previous + amount);
    this.byKey.set(key, { damage, blockId });
    return damage;
  }

  /** 方块已破坏或世界重置时清除 */
  clear(key: string): void {
    this.byKey.delete(key);
  }

  clearAll(): void {
    this.byKey.clear();
  }

  /** 是否有任何未完成损伤（调试/测试） */
  get size(): number {
    return this.byKey.size;
  }

  /**
   * 全部有损伤的体素快照（供常驻裂纹 overlay）。
   * key 格式与 voxelKey 一致：`x,y,z`
   */
  snapshot(): ReadonlyArray<{
    key: string;
    voxel: readonly [number, number, number];
    damage: number;
    blockId: number;
  }> {
    const out: Array<{
      key: string;
      voxel: readonly [number, number, number];
      damage: number;
      blockId: number;
    }> = [];
    for (const [key, entry] of this.byKey) {
      if (!(entry.damage > 0)) continue;
      const parts = key.split(",");
      if (parts.length !== 3) continue;
      const x = Number(parts[0]);
      const y = Number(parts[1]);
      const z = Number(parts[2]);
      if (
        !Number.isInteger(x) ||
        !Number.isInteger(y) ||
        !Number.isInteger(z)
      ) {
        continue;
      }
      out.push({
        key,
        voxel: [x, y, z],
        damage: entry.damage,
        blockId: entry.blockId,
      });
    }
    return out;
  }
}
