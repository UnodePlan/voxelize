import { LOCAL_INVENTORY_SLOTS } from "./state";

export interface LocalInputActions {
  closeInventory(): void;
  cycleSlot(direction: -1 | 1): void;
  dropSelectedSlot(): void;
  isGameplayActive(): boolean;
  isInventoryOpen(): boolean;
  isPointerLocked(): boolean;
  selectSlot(slot: number): void;
  toggleHelp(): void;
  toggleInventory(): void;
}

export class LocalInputController {
  private miningHeld = false;
  /** 右键放置（对齐 Voxelize demo / 原版） */
  private placeHeld = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly actions: LocalInputActions,
  ) {
    canvas.addEventListener("mousedown", this.handleMouseDown);
    canvas.addEventListener("contextmenu", this.preventContextMenu);
    canvas.addEventListener("wheel", this.handleWheel, { passive: false });
    document.addEventListener("mouseup", this.handleMouseUp);
    document.addEventListener("keydown", this.handleKeyDown);
    document.addEventListener("visibilitychange", this.handleVisibility);
  }

  get primaryHeld(): boolean {
    return this.miningHeld;
  }

  get secondaryHeld(): boolean {
    return this.placeHeld;
  }

  cancelMining(): void {
    this.miningHeld = false;
  }

  cancelPlace(): void {
    this.placeHeld = false;
  }

  dispose(): void {
    this.cancelMining();
    this.cancelPlace();
    this.canvas.removeEventListener("mousedown", this.handleMouseDown);
    this.canvas.removeEventListener("contextmenu", this.preventContextMenu);
    this.canvas.removeEventListener("wheel", this.handleWheel);
    document.removeEventListener("mouseup", this.handleMouseUp);
    document.removeEventListener("keydown", this.handleKeyDown);
    document.removeEventListener("visibilitychange", this.handleVisibility);
  }

  private readonly handleMouseDown = (event: MouseEvent): void => {
    if (
      !this.actions.isGameplayActive() ||
      this.actions.isInventoryOpen() ||
      !this.actions.isPointerLocked()
    ) {
      return;
    }
    if (event.button === 0) {
      this.miningHeld = true;
    } else if (event.button === 2) {
      // 右键：放置（contextmenu 已 preventDefault）
      this.placeHeld = true;
    }
  };

  private readonly handleMouseUp = (event: MouseEvent): void => {
    if (event.button === 0) this.cancelMining();
    if (event.button === 2) this.cancelPlace();
  };

  private readonly handleKeyDown = (event: KeyboardEvent): void => {
    if (event.repeat || !this.actions.isGameplayActive()) return;

    // Escape：优先关背包，再交给指针锁释放
    if (event.code === "Escape") {
      if (this.actions.isInventoryOpen()) {
        event.preventDefault();
        this.actions.closeInventory();
        this.cancelMining();
        this.cancelPlace();
      }
      return;
    }

    // E：开关 Lab 风格物品栏（不要求指针锁）
    if (event.code === "KeyE") {
      event.preventDefault();
      this.actions.toggleInventory();
      this.cancelMining();
      this.cancelPlace();
      return;
    }

    if (event.code === "KeyH") {
      if (!this.actions.isInventoryOpen()) this.actions.toggleHelp();
      return;
    }

    // 背包打开时仍可用数字键选槽；挖掘相关需指针锁
    if (this.actions.isInventoryOpen()) {
      const slot = slotForCode(event.code);
      if (slot !== null) this.actions.selectSlot(slot);
      return;
    }

    if (!this.actions.isPointerLocked()) return;
    const slot = slotForCode(event.code);
    if (slot !== null) {
      this.actions.selectSlot(slot);
    } else if (event.code === "KeyQ") {
      this.actions.dropSelectedSlot();
    }
  };

  private readonly handleWheel = (event: WheelEvent): void => {
    if (
      !this.actions.isGameplayActive() ||
      this.actions.isInventoryOpen() ||
      !this.actions.isPointerLocked()
    )
      return;
    event.preventDefault();
    this.actions.cycleSlot(event.deltaY > 0 ? 1 : -1);
  };

  private readonly handleVisibility = (): void => {
    if (document.hidden) {
      this.cancelMining();
      this.cancelPlace();
    }
  };

  private readonly preventContextMenu = (event: Event): void =>
    event.preventDefault();
}

function slotForCode(code: string): number | null {
  if (/^Digit[1-9]$/.test(code)) return Number(code.at(-1)) - 1;
  if (code === "Digit0") return 9;
  if (code === "Minus") return 10;
  if (code === "Equal") return 11;
  return null;
}

export function cycleInventorySlot(current: number, direction: -1 | 1): number {
  return (current + direction + LOCAL_INVENTORY_SLOTS) % LOCAL_INVENTORY_SLOTS;
}
