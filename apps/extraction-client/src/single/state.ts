import { applyDamage, clampHealth, fullHealth } from "./combat";

export const LOCAL_INVENTORY_SLOTS = 12;
export const LOCAL_STACK_LIMIT = 64;
export const LOCAL_EXTRACTION_REQUIRED_MS = 5_000;
/** 快捷栏前 3 格固定工具：0 空手 / 1 镐 / 2 剑；资源只进入后续槽 */
export const LOCAL_TOOL_HOTBAR_SLOTS = 3;

/**
 * 单机可进背包的全部资源（与生产 ResourceCounts 解耦）。
 * 每种可破坏方块应对应一项，并有 HUD/掉落图标。
 */
export const LOCAL_RESOURCE_KEYS = [
  "dirt",
  "grass",
  "stone",
  "planks",
  "leaves",
  "gold",
  "diamond",
] as const;

export type LocalResourceKey = (typeof LOCAL_RESOURCE_KEYS)[number];

export type LocalResourceCounts = Record<LocalResourceKey, number>;

/** 结果/图鉴展示名 */
export const LOCAL_RESOURCE_LABELS: Readonly<Record<LocalResourceKey, string>> =
  {
    dirt: "泥土",
    grass: "草地",
    stone: "岩石",
    planks: "木板",
    leaves: "树叶",
    gold: "黄金矿",
    diamond: "钻石矿",
  };

export function emptyLocalResourceCounts(): LocalResourceCounts {
  return {
    dirt: 0,
    grass: 0,
    stone: 0,
    planks: 0,
    leaves: 0,
    gold: 0,
    diamond: 0,
  };
}

export function isLocalResourceKey(value: string): value is LocalResourceKey {
  return (LOCAL_RESOURCE_KEYS as readonly string[]).includes(value);
}

export function isToolHotbarSlot(slot: number): boolean {
  return Number.isInteger(slot) && slot >= 0 && slot < LOCAL_TOOL_HOTBAR_SLOTS;
}

export type LocalVoxel = readonly [number, number, number];
export type LocalGamePhase = "loading" | "playing" | "extracted" | "error";
export type LocalHintMode = "controls" | "resume" | "hidden";
export type LocalInventoryTab = "items" | "blocks";

export interface LocalInventorySlot {
  quantity: number;
  resource: LocalResourceKey;
}

export interface LocalMiningState {
  elapsedMs: number;
  requiredMs: number;
  /** 破坏后进背包的资源；不可收获时为 null */
  resource: LocalResourceKey | null;
  /** HUD 显示名 */
  displayName: string;
  target: LocalVoxel;
  targetKey: string;
}

export interface LocalResult {
  elapsedMs: number;
  resources: LocalResourceCounts;
}

export interface LocalGameState {
  elapsedMs: number;
  error: string | null;
  extractionElapsedMs: number;
  hint: LocalHintMode;
  inventory: ReadonlyArray<LocalInventorySlot | null>;
  /** Lab 风格：按 E 打开的完整物品栏 */
  inventoryOpen: boolean;
  inventoryTab: LocalInventoryTab;
  mining: LocalMiningState | null;
  notice: string | null;
  phase: LocalGamePhase;
  result: LocalResult | null;
  selectedSlot: number;
  /**
   * 玩家生命（半心为单位，满血 20 = 10 星）。
   * 见 combat.ts
   */
  playerHealth: number;
}

export type LocalGameAction =
  | { type: "WORLD_READY" }
  | { type: "FRAME"; deltaMs: number; insideExtraction: boolean }
  | { type: "POINTER_LOCKED" }
  | { type: "POINTER_UNLOCKED" }
  | { type: "TOGGLE_HELP" }
  | { type: "TOGGLE_INVENTORY" }
  | { type: "CLOSE_INVENTORY" }
  | { type: "SET_INVENTORY_TAB"; tab: LocalInventoryTab }
  | {
      type: "MINING_STARTED";
      target: LocalVoxel;
      resource: LocalResourceKey | null;
      displayName: string;
      requiredMs: number;
      /** 续挖时从上一次耐久恢复的已用时（默认 0） */
      elapsedMs?: number;
    }
  | {
      type: "MINING_ADVANCED";
      deltaMs: number;
      targetKey: string;
      /** 若给出则用绝对值覆盖（来自世界耐久表） */
      elapsedMs?: number;
    }
  | { type: "MINING_CANCELLED" }
  | {
      type: "INVENTORY_REPLACED";
      inventory: ReadonlyArray<LocalInventorySlot | null>;
    }
  | { type: "SLOT_SELECTED"; slot: number }
  | { type: "INVENTORY_SWAP"; from: number; to: number }
  | { type: "NOTICE_SET"; notice: string | null }
  | { type: "PLAYER_DAMAGED"; amount: number }
  | { type: "PLAYER_HEALED"; health?: number }
  | { type: "FAILED"; message: string };

export function createInitialLocalGameState(): LocalGameState {
  return {
    elapsedMs: 0,
    error: null,
    extractionElapsedMs: 0,
    hint: "controls",
    inventory: emptyInventory(),
    inventoryOpen: false,
    inventoryTab: "items",
    mining: null,
    notice: null,
    phase: "loading",
    result: null,
    selectedSlot: 0,
    playerHealth: fullHealth(),
  };
}

export function reduceLocalGameState(
  state: LocalGameState,
  action: LocalGameAction,
): LocalGameState {
  switch (action.type) {
    case "WORLD_READY":
      return state.phase === "loading" ? { ...state, phase: "playing" } : state;
    case "FRAME":
      return advanceFrame(state, action.deltaMs, action.insideExtraction);
    case "POINTER_LOCKED":
      return state.phase === "playing" ? { ...state, hint: "hidden" } : state;
    case "POINTER_UNLOCKED":
      return state.phase === "playing"
        ? { ...state, hint: "resume", mining: null }
        : state;
    case "TOGGLE_HELP":
      if (state.phase !== "playing") return state;
      return {
        ...state,
        hint: state.hint === "controls" ? "hidden" : "controls",
      };
    case "TOGGLE_INVENTORY":
      if (state.phase !== "playing") return state;
      return {
        ...state,
        inventoryOpen: !state.inventoryOpen,
        mining: null,
      };
    case "CLOSE_INVENTORY":
      return state.inventoryOpen
        ? { ...state, inventoryOpen: false, mining: null }
        : state;
    case "SET_INVENTORY_TAB":
      if (state.phase !== "playing" || !state.inventoryOpen) return state;
      if (action.tab !== "items" && action.tab !== "blocks") return state;
      return { ...state, inventoryTab: action.tab };
    case "MINING_STARTED":
      if (
        state.phase !== "playing" ||
        state.inventoryOpen ||
        action.requiredMs <= 0 ||
        action.displayName.trim() === ""
      )
        return state;
      {
        const resumed =
          typeof action.elapsedMs === "number" &&
          Number.isFinite(action.elapsedMs) &&
          action.elapsedMs > 0
            ? Math.min(action.requiredMs, action.elapsedMs)
            : 0;
        return {
          ...state,
          mining: {
            elapsedMs: resumed,
            requiredMs: action.requiredMs,
            resource: action.resource,
            displayName: action.displayName,
            target: action.target,
            targetKey: voxelKey(action.target),
          },
        };
      }
    case "MINING_ADVANCED": {
      const mining = state.mining;
      if (
        state.phase !== "playing" ||
        mining === null ||
        mining.targetKey !== action.targetKey ||
        mining.elapsedMs >= mining.requiredMs
      ) {
        return state;
      }
      const nextElapsed =
        typeof action.elapsedMs === "number" &&
        Number.isFinite(action.elapsedMs)
          ? Math.min(mining.requiredMs, Math.max(0, action.elapsedMs))
          : Math.min(
              mining.requiredMs,
              mining.elapsedMs + validDelta(action.deltaMs),
            );
      return {
        ...state,
        mining: {
          ...mining,
          elapsedMs: nextElapsed,
        },
      };
    }
    case "MINING_CANCELLED":
      return state.mining === null ? state : { ...state, mining: null };
    case "INVENTORY_REPLACED":
      return action.inventory.length === LOCAL_INVENTORY_SLOTS
        ? { ...state, inventory: [...action.inventory] }
        : state;
    case "SLOT_SELECTED":
      return Number.isInteger(action.slot) &&
        action.slot >= 0 &&
        action.slot < LOCAL_INVENTORY_SLOTS
        ? { ...state, selectedSlot: action.slot }
        : state;
    case "INVENTORY_SWAP": {
      if (state.phase !== "playing") return state;
      const swapped = swapInventorySlots(
        state.inventory,
        action.from,
        action.to,
      );
      if (swapped === state.inventory) return state;
      // 选中跟随被拖动物品到目标槽
      return {
        ...state,
        inventory: swapped,
        selectedSlot: action.to,
        mining: null,
      };
    }
    case "NOTICE_SET":
      return { ...state, notice: action.notice };
    case "PLAYER_DAMAGED":
      if (state.phase !== "playing") return state;
      return {
        ...state,
        playerHealth: applyDamage(state.playerHealth, action.amount),
      };
    case "PLAYER_HEALED":
      return {
        ...state,
        playerHealth:
          typeof action.health === "number"
            ? clampHealth(action.health)
            : fullHealth(),
      };
    case "FAILED":
      return {
        ...state,
        error: action.message,
        inventoryOpen: false,
        mining: null,
        phase: "error",
      };
  }
}

export function addResource(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
  resource: LocalResourceKey,
  quantity: number,
): {
  inventory: ReadonlyArray<LocalInventorySlot | null>;
  remainder: number;
} {
  assertInventory(inventory);
  assertQuantity(quantity);
  const next = inventory.map((slot) => (slot === null ? null : { ...slot }));
  let remainder = quantity;

  // 先叠已有同类堆；工具槽不参与资源堆叠
  for (let index = LOCAL_TOOL_HOTBAR_SLOTS; index < next.length; index += 1) {
    const slot = next[index];
    if (slot?.resource !== resource || slot.quantity >= LOCAL_STACK_LIMIT) {
      continue;
    }
    const accepted = Math.min(LOCAL_STACK_LIMIT - slot.quantity, remainder);
    slot.quantity += accepted;
    remainder -= accepted;
    if (remainder === 0) return { inventory: next, remainder };
  }

  for (
    let index = LOCAL_TOOL_HOTBAR_SLOTS;
    index < next.length && remainder > 0;
    index += 1
  ) {
    if (next[index] !== null) continue;
    const accepted = Math.min(LOCAL_STACK_LIMIT, remainder);
    next[index] = { quantity: accepted, resource };
    remainder -= accepted;
  }

  return { inventory: next, remainder };
}

export function dropInventorySlot(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
  slot: number,
): {
  dropped: LocalInventorySlot | null;
  inventory: ReadonlyArray<LocalInventorySlot | null>;
} {
  assertInventory(inventory);
  // 工具槽不是资源格，Q 丢弃不生效
  if (
    !Number.isInteger(slot) ||
    slot < 0 ||
    slot >= inventory.length ||
    isToolHotbarSlot(slot)
  ) {
    return { dropped: null, inventory };
  }
  const dropped = inventory[slot];
  if (dropped === null) return { dropped: null, inventory };
  const next = [...inventory];
  next[slot] = null;
  return { dropped: { ...dropped }, inventory: next };
}

/**
 * 从指定槽扣 1 个资源（放置用）。工具槽 / 空槽失败。
 */
export function consumeInventorySlot(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
  slot: number,
): {
  inventory: ReadonlyArray<LocalInventorySlot | null>;
  resource: LocalResourceKey | null;
} {
  assertInventory(inventory);
  if (
    !Number.isInteger(slot) ||
    slot < 0 ||
    slot >= inventory.length ||
    isToolHotbarSlot(slot)
  ) {
    return { inventory, resource: null };
  }
  const current = inventory[slot];
  if (current === null || current.quantity <= 0) {
    return { inventory, resource: null };
  }
  const next = [...inventory];
  if (current.quantity <= 1) {
    next[slot] = null;
  } else {
    next[slot] = { resource: current.resource, quantity: current.quantity - 1 };
  }
  return { inventory: next, resource: current.resource };
}

/** 拖拽整理：交换两个槽；同源、非法索引或工具槽则原样返回。 */
export function swapInventorySlots(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
  from: number,
  to: number,
): ReadonlyArray<LocalInventorySlot | null> {
  assertInventory(inventory);
  if (
    !Number.isInteger(from) ||
    !Number.isInteger(to) ||
    from < 0 ||
    to < 0 ||
    from >= inventory.length ||
    to >= inventory.length ||
    from === to ||
    isToolHotbarSlot(from) ||
    isToolHotbarSlot(to)
  ) {
    return inventory;
  }
  if (inventory[from] === null && inventory[to] === null) return inventory;
  const next = inventory.map((slot) => (slot === null ? null : { ...slot }));
  const temp = next[from];
  next[from] = next[to];
  next[to] = temp;
  return next;
}

export function inventoryResourceCounts(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
): LocalResourceCounts {
  assertInventory(inventory);
  return inventory.reduce<LocalResourceCounts>((counts, slot) => {
    if (slot !== null) counts[slot.resource] += slot.quantity;
    return counts;
  }, emptyLocalResourceCounts());
}

export function voxelKey(voxel: LocalVoxel): string {
  return voxel.join(",");
}

function advanceFrame(
  state: LocalGameState,
  deltaMs: number,
  insideExtraction: boolean,
): LocalGameState {
  if (state.phase !== "playing") return state;
  const delta = validDelta(deltaMs);
  const elapsedMs = state.elapsedMs + delta;
  const extractionElapsedMs = insideExtraction
    ? Math.min(LOCAL_EXTRACTION_REQUIRED_MS, state.extractionElapsedMs + delta)
    : 0;
  if (extractionElapsedMs < LOCAL_EXTRACTION_REQUIRED_MS) {
    return { ...state, elapsedMs, extractionElapsedMs };
  }
  return {
    ...state,
    elapsedMs,
    extractionElapsedMs,
    hint: "hidden",
    inventoryOpen: false,
    mining: null,
    phase: "extracted",
    result: {
      elapsedMs,
      resources: inventoryResourceCounts(state.inventory),
    },
  };
}

function emptyInventory(): ReadonlyArray<null> {
  return Array.from({ length: LOCAL_INVENTORY_SLOTS }, () => null);
}

function validDelta(deltaMs: number): number {
  return Number.isFinite(deltaMs) && deltaMs > 0 ? deltaMs : 0;
}

function assertQuantity(quantity: number): void {
  if (!Number.isSafeInteger(quantity) || quantity <= 0) {
    throw new Error("本地资源数量必须是正安全整数");
  }
}

function assertInventory(
  inventory: ReadonlyArray<LocalInventorySlot | null>,
): void {
  if (inventory.length !== LOCAL_INVENTORY_SLOTS) {
    throw new Error(`本地背包必须恰好包含 ${LOCAL_INVENTORY_SLOTS} 个槽位`);
  }
}
