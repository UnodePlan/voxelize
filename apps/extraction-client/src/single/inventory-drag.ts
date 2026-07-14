import { isToolHotbarSlot, LOCAL_INVENTORY_SLOTS } from "./state";

/** 背包打开时槽位拖拽整理（热栏 + My Items 网格）。 */
export class InventoryDragController {
  private dragFrom: number | null = null;
  private dragPointerId: number | null = null;
  private dragMoved = false;

  constructor(
    private readonly shell: HTMLElement,
    private readonly dragGhost: HTMLElement,
    private readonly allSlots: () => HTMLElement[],
    private readonly getResource: (
      index: number,
    ) => { resource: string; quantity: number } | null,
    private readonly onSelect: (slot: number) => void,
    private readonly onSwap: (from: number, to: number) => void,
  ) {
    this.shell.addEventListener("pointerdown", this.handlePointerDown);
    window.addEventListener("pointermove", this.handlePointerMove);
    window.addEventListener("pointerup", this.handlePointerUp);
    window.addEventListener("pointercancel", this.handlePointerUp);
  }

  dispose(): void {
    this.endDrag();
    this.shell.removeEventListener("pointerdown", this.handlePointerDown);
    window.removeEventListener("pointermove", this.handlePointerMove);
    window.removeEventListener("pointerup", this.handlePointerUp);
    window.removeEventListener("pointercancel", this.handlePointerUp);
  }

  private readonly handlePointerDown = (event: PointerEvent): void => {
    if (event.button !== 0) return;
    if (this.shell.dataset.inventory !== "open") return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const slot = target.closest<HTMLElement>(
      ".single-inv-items .single-slot, .single-hotbar .single-slot",
    );
    if (slot === null) return;
    const index = Number(slot.dataset.index);
    if (!Number.isInteger(index) || index < 0 || index >= LOCAL_INVENTORY_SLOTS)
      return;
    // 固定工具槽只切换握持，不参与资源拖拽
    if (isToolHotbarSlot(index)) {
      this.onSelect(index);
      return;
    }
    const content = this.getResource(index);
    if (content === null) {
      this.onSelect(index);
      return;
    }
    event.preventDefault();
    this.dragFrom = index;
    this.dragPointerId = event.pointerId;
    this.dragMoved = false;
    this.onSelect(index);
    this.shell.dataset.dragging = "true";
    slot.classList.add("is-drag-source");
    this.dragGhost.hidden = false;
    this.dragGhost.dataset.resource = content.resource;
    this.dragGhost.textContent =
      content.quantity > 1 ? String(content.quantity) : "";
    this.moveGhost(event.clientX, event.clientY);
  };

  private readonly handlePointerMove = (event: PointerEvent): void => {
    if (this.dragFrom === null || event.pointerId !== this.dragPointerId) return;
    this.dragMoved = true;
    this.moveGhost(event.clientX, event.clientY);
    this.highlightDropTarget(slotIndexFromPoint(event.clientX, event.clientY));
  };

  private readonly handlePointerUp = (event: PointerEvent): void => {
    if (this.dragFrom === null || event.pointerId !== this.dragPointerId) return;
    const from = this.dragFrom;
    const to = slotIndexFromPoint(event.clientX, event.clientY);
    const moved = this.dragMoved;
    this.endDrag();
    if (
      to !== null &&
      to !== from &&
      moved &&
      !isToolHotbarSlot(from) &&
      !isToolHotbarSlot(to)
    ) {
      this.onSwap(from, to);
    } else if (to !== null) {
      this.onSelect(to);
    }
  };

  private moveGhost(x: number, y: number): void {
    this.dragGhost.style.transform = `translate(${x + 8}px, ${y + 8}px)`;
  }

  private highlightDropTarget(index: number | null): void {
    for (const el of this.allSlots()) {
      el.classList.toggle(
        "is-drop-target",
        index !== null && Number(el.dataset.index) === index,
      );
    }
  }

  private endDrag(): void {
    this.dragFrom = null;
    this.dragPointerId = null;
    this.dragMoved = false;
    this.dragGhost.hidden = true;
    this.dragGhost.dataset.resource = "empty";
    this.dragGhost.textContent = "";
    this.shell.dataset.dragging = "false";
    for (const el of this.allSlots()) {
      el.classList.remove("is-drag-source", "is-drop-target");
    }
  }
}

function slotIndexFromPoint(x: number, y: number): number | null {
  const el = document.elementFromPoint(x, y);
  if (!(el instanceof Element)) return null;
  const slot = el.closest<HTMLElement>(
    ".single-inv-items .single-slot, .single-hotbar .single-slot",
  );
  if (slot === null) return null;
  const index = Number(slot.dataset.index);
  return Number.isInteger(index) &&
    index >= 0 &&
    index < LOCAL_INVENTORY_SLOTS
    ? index
    : null;
}
