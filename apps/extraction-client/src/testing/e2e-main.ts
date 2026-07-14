import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  type GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import type { MatchResult, MatchResultStatus } from "../api/models";
import type { AppState } from "../app/state";
import { INITIAL_APP_STATE } from "../app/state";
import { VoxelBackdrop } from "../game/scene";
import { mountProductShell } from "../ui/shell";
import { renderProductView } from "../ui/view";

const MATCH_ID = "11111111-1111-4111-8111-111111111111";
const PLAYER_ID = "22222222-2222-4222-8222-222222222222";
const manifest = decodeExtractionManifest(manifestJson);

export type E2eScreen = "auth" | "lobby" | "match" | "result";

export interface ExtractionE2eBridge {
  setHalfHearts(value: number): void;
  setResult(status: MatchResultStatus): void;
  show(screen: E2eScreen): void;
  snapshot(): {
    canvasFrame: number;
    canvasHeight: number;
    canvasWidth: number;
    screen: string;
  };
}

declare global {
  interface Window {
    __VOXEL_EXTRACTION_E2E__?: ExtractionE2eBridge;
  }
}

export function startE2eClient(root: HTMLElement): void {
  const parameters = new URLSearchParams(window.location.search);
  const singlePlayer = parameters.get("single") === "1";
  const elements = mountProductShell(root);
  let state = lobbyState();
  const updateSinglePlayer = (next: AppState): void => {
    if (!singlePlayer) return;
    state = next;
    render();
  };
  const scene = new VoxelBackdrop(elements.canvas, {
    attack: () =>
      updateSinglePlayer({
        ...state,
        notice: "单机调试：当前没有本地战斗目标。",
      }),
    dropSlot: (slot) => updateSinglePlayer(dropLocalSlot(state, slot)),
    getManifest: () => manifest,
    mining: (action) => {
      if (action === "start") updateSinglePlayer(mineLocalDirt(state));
    },
    movement: () => undefined,
    onError: () => undefined,
    onWorldReady: () => undefined,
    sendWorldPacket: () => undefined,
  });

  const render = () => {
    renderProductView(state, elements);
    scene.setMode(
      state.screen === "match"
        ? "match"
        : state.screen === "result"
          ? "result"
          : "lobby",
    );
  };
  const bridge: ExtractionE2eBridge = {
    setHalfHearts: (value) => {
      state = withHalfHearts(state, value);
      render();
    },
    setResult: (status) => {
      state = { ...state, screen: "result", result: matchResult(status) };
      render();
    },
    show: (screen) => {
      state = screenState(screen);
      render();
    },
    snapshot: () => ({
      canvasFrame: Number(elements.canvas.dataset.renderFrame ?? 0),
      canvasHeight: elements.canvas.height,
      canvasWidth: elements.canvas.width,
      screen: state.screen,
    }),
  };
  window.__VOXEL_EXTRACTION_E2E__ = bridge;
  const requestedScreen = parameters.get("screen");
  if (isE2eScreen(requestedScreen)) state = screenState(requestedScreen);
  if (parameters.get("single") === "1") {
    state = {
      ...screenState("match"),
      notice: "单机调试：点击场景或按 E 挖掘，按 Q 丢弃首个资源槽，空格测试攻击。刷新即清空。",
    };
    const mine = () => updateSinglePlayer(mineLocalDirt(state));
    elements.canvas.addEventListener("click", mine);
    window.addEventListener("keydown", (event) => {
      if (event.repeat) return;
      if (event.code === "KeyE") mine();
      if (event.code === "KeyQ") {
        const firstOccupied = state.gameplay?.inventory.slots.findIndex(
          (slot) => slot !== null,
        );
        if (firstOccupied !== undefined && firstOccupied >= 0) {
          updateSinglePlayer(dropLocalSlot(state, firstOccupied));
        }
      }
      if (event.code === "Space") {
        event.preventDefault();
        updateSinglePlayer({
          ...state,
          notice: "单机调试：当前没有本地战斗目标。",
        });
      }
    });
  }
  const requestedResult = parameters.get("result");
  if (isResultStatus(requestedResult)) {
    state = {
      ...state,
      screen: "result",
      result: matchResult(requestedResult),
    };
  }
  const requestedHealth = Number(parameters.get("halfHearts"));
  if (parameters.has("halfHearts")) {
    state = withHalfHearts(state, requestedHealth);
  }
  render();
}

function mineLocalDirt(state: AppState): AppState {
  const gameplay = state.gameplay ?? gameplayState();
  const slots = [...gameplay.inventory.slots];
  const dirtSlot = slots.findIndex((slot) => slot?.resource === "dirt");
  if (dirtSlot >= 0 && slots[dirtSlot] !== null) {
    const current = slots[dirtSlot];
    if (current.quantity >= 64) {
      return { ...state, notice: "单机调试：泥土堆叠已达到 64。" };
    }
    slots[dirtSlot] = { ...current, quantity: current.quantity + 1 };
  } else {
    const emptySlot = slots.findIndex((slot) => slot === null);
    if (emptySlot < 0) {
      return { ...state, notice: "单机调试：背包已满。" };
    }
    slots[emptySlot] = { resource: "dirt", quantity: 1 };
  }
  return withLocalSlots(state, gameplay, slots, "单机调试：获得 1 个泥土。刷新页面后清空。 ");
}

function dropLocalSlot(state: AppState, slot: number): AppState {
  const gameplay = state.gameplay ?? gameplayState();
  if (!Number.isInteger(slot) || slot < 0 || slot >= gameplay.inventory.slots.length) {
    return state;
  }
  const slots = [...gameplay.inventory.slots];
  if (slots[slot] === null) return state;
  slots[slot] = null;
  return withLocalSlots(state, gameplay, slots, "单机调试：已丢弃该资源槽。刷新页面后重置。 ");
}

function withLocalSlots(
  state: AppState,
  gameplay: GameplayStateData,
  slots: GameplayStateData["inventory"]["slots"],
  notice: string,
): AppState {
  return {
    ...state,
    notice: notice.trim(),
    gameplay: {
      ...gameplay,
      inventory: {
        ...gameplay.inventory,
        slots,
        revision: gameplay.inventory.revision + 1,
      },
    },
  };
}

function withHalfHearts(state: AppState, value: number): AppState {
  if (!Number.isInteger(value) || value < 0 || value > 20) {
    throw new Error("half hearts must be in 0..20");
  }
  const gameplay = state.gameplay ?? gameplayState();
  return {
    ...state,
    screen: "match",
    gameplay: {
      ...gameplay,
      health: {
        ...gameplay.health,
        revision: gameplay.health.revision + 1,
        data:
          value === 0
            ? { status: "dead", currentHalfHearts: 0, maxHalfHearts: 20 }
            : { status: "alive", currentHalfHearts: value, maxHalfHearts: 20 },
      },
    },
  };
}

function isE2eScreen(value: string | null): value is E2eScreen {
  return value !== null && ["auth", "lobby", "match", "result"].includes(value);
}

function isResultStatus(value: string | null): value is MatchResultStatus {
  return (
    value !== null &&
    [
      "pendingReconciliation",
      "extracted",
      "dead",
      "timedOut",
      "aborted",
    ].includes(value)
  );
}

function screenState(screen: E2eScreen): AppState {
  if (screen === "auth") {
    return {
      ...INITIAL_APP_STATE,
      screen: "unauthenticated",
      manifest,
      wallet: {
        configured: true,
        connected: false,
        address: null,
        chainId: null,
      },
    };
  }
  if (screen === "lobby") return lobbyState();
  if (screen === "result") {
    return {
      ...lobbyState(),
      screen: "result",
      result: matchResult("extracted"),
    };
  }
  return {
    ...lobbyState(),
    screen: "match",
    activeMatchId: MATCH_ID,
    worldName: `match:v1:${MATCH_ID}`,
    connection: "online",
    gameplay: gameplayState(),
  };
}

function lobbyState(): AppState {
  return {
    ...INITIAL_APP_STATE,
    screen: "lobby",
    manifest,
    session: {
      address: "0x1111111111111111111111111111111111111111",
      chainId: 1,
    },
    wallet: {
      configured: true,
      connected: true,
      address: "0x1111111111111111111111111111111111111111",
      chainId: 1,
    },
    queue: { status: "idle" },
    warehouse: {
      resources: { dirt: 184, gold: 37, diamond: 8 },
      stats: {
        totalResourcesExtracted: 229,
        totalExtractionValue: 824,
        successfulExtractions: 4,
        highestSingleMatchValue: 318,
      },
    },
    result: matchResult("extracted"),
  };
}

function gameplayState(): GameplayStateData {
  const now = Math.floor(Date.now() / 1_000);
  return {
    matchId: MATCH_ID,
    inventory: {
      slots: [
        { resource: "dirt", quantity: 32 },
        { resource: "gold", quantity: 8 },
        { resource: "diamond", quantity: 2 },
        ...Array.from({ length: 9 }, () => null),
      ],
      revision: 4,
      frozen: false,
      lastDropSequence: 3,
    },
    equipment: { pickaxe: "basic_pickaxe", meleeWeapon: "basic_melee_weapon" },
    mining: {
      protocolVersion: manifest.protocolVersion,
      type: "state",
      matchId: MATCH_ID,
      stream: "mining",
      revision: 2,
      data: { status: "idle", acceptedSequence: 2, reason: "completed" },
    },
    extraction: {
      protocolVersion: manifest.protocolVersion,
      type: "state",
      matchId: MATCH_ID,
      stream: "extraction",
      revision: 0,
      data: {
        status: "hidden",
        extractionOpenAtUnixSeconds: now + 180,
        hardDeadlineUnixSeconds: now + 420,
      },
    },
    health: {
      protocolVersion: manifest.protocolVersion,
      type: "state",
      matchId: MATCH_ID,
      stream: "health",
      revision: 1,
      data: { status: "alive", currentHalfHearts: 17, maxHalfHearts: 20 },
    },
    attack: { revision: 2, acceptedSequence: 2 },
    deathResult: null,
  };
}

function matchResult(status: MatchResultStatus): MatchResult {
  const extracted = status === "extracted";
  const dead = status === "dead";
  const timedOut = status === "timedOut";
  return {
    matchId: MATCH_ID,
    status,
    publicPlayerId: PLAYER_ID,
    terminalCause: dead ? "melee" : timedOut ? "hardDeadline" : null,
    killerPublicPlayerId: dead ? "33333333-3333-4333-8333-333333333333" : null,
    terminalAt: dead || timedOut ? "2026-07-13T08:12:00Z" : null,
    survivedMs: dead || timedOut ? 532_000 : null,
    stats: {
      mined: { dirt: 23, gold: 7, diamond: 2 },
      pickedUp: { dirt: 6, gold: 3, diamond: 0 },
      lost: extracted
        ? { dirt: 0, gold: 0, diamond: 0 }
        : { dirt: 29, gold: 10, diamond: 2 },
    },
    settlement: extracted
      ? {
          settlementId: "44444444-4444-4444-8444-444444444444",
          resources: { dirt: 29, gold: 10, diamond: 2 },
          totalValue: 318,
          configVersion: "extraction-pvp-v1",
          committedAt: "2026-07-13T08:12:08Z",
        }
      : null,
  };
}
