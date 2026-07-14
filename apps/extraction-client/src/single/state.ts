import type { ResourceCounts } from "../api/models";

export const LOCAL_INVENTORY_SLOTS = 12;
export const LOCAL_STACK_LIMIT = 64;
export const LOCAL_EXTRACTION_REQUIRED_MS = 3_000;

export type LocalResourceKey = keyof ResourceCounts;
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
  resource: LocalResourceKey;
  target: LocalVoxel;
  targetKey: string;
}

export interface LocalResult {
  elapsedMs: number;
  resources: ResourceCounts;
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
      resource: LocalResourceKey;
      requiredMs: number;
    }
  | { type: "MINING_ADVANCED"; deltaMs: number; targetKey: string }
  | { type: "MINING_CANCELLED" }
  | {
      type: "INVENTORY_REPLACED";
      inventory: ReadonlyArray<LocalInventorySlot | null>;
    }
  | { type: "SLOT_SELECTED"; slot: number }
  | { type: "INVENTORY_SWAP"; from: number; to: number }
  | { type: "NOTICE_SET"; notice: string | null }
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
        action.requiredMs <= 0
      )
        return state;
      return {
        ...state,
        mining: {
          elapsedMs: 0,
          requiredMs: action.requiredMs,
          resource: action.resource,
          target: action.target,
          targetKey: voxelKey(action.target),
        },
      };
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
      return {
        ...state,
        mining: {
          ...mining,
          elapsedMs: Math.min(
            mining.requiredMs,
            mining.elapsedMs + validDelta(action.deltaMs),
          ),
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

  for (const slot of next) {
    if (slot?.resource !== resource || slot.quantity >= LOCAL_STACK_LIMIT) {
      continue;
    }
    const accepted = Math.min(LOCAL_STACK_LIMIT - slot.quantity, remainder);
    slot.quantity += accepted;
    remainder -= accepted;
    if (remainder === 0) return { inventory: next, remainder };
  }

  for (let index = 0; index < next.length && remainder > 0; index += 1) {
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
  if (!Number.isInteger(slot) || slot < 0 || slot >= inventory.length) {
    return { dropped: null, inventory };
  }
  const dropped = inventory[slot];
  if (dropped === null) return { dropped: null, inventory };
  const next = [...inventory];
  next[slot] = null;
  return { dropped: { ...dropped }, inventory: next };
}

/** 拖拽整理：交换两个槽；同源或非法索引则原样返回。 */
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
    from === to
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
): ResourceCounts {
  assertInventory(inventory);
  return inventory.reduce<ResourceCounts>(
    (counts, slot) => {
      if (slot !== null) counts[slot.resource] += slot.quantity;
      return counts;
    },
    { dirt: 0, gold: 0, diamond: 0 },
  );
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
