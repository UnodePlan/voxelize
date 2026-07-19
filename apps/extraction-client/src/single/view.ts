import { heartsFromHealth, type HeartIcon } from "./combat";
import { InventoryDragController } from "./inventory-drag";
import type {
  LocalGameState,
  LocalInventoryTab,
  LocalResourceKey,
} from "./state";
import {
  LOCAL_EXTRACTION_REQUIRED_MS,
  LOCAL_INVENTORY_SLOTS,
  LOCAL_RESOURCE_KEYS,
  LOCAL_RESOURCE_LABELS,
} from "./state";

export interface LocalViewFrame {
  insideExtraction: boolean;
  targetName: string | null;
  /** 本局地图风格名，如「荒漠 · 烈日」 */
  styleLabel?: string | null;
}

export interface LocalViewActions {
  closeInventory(): void;
  selectSlot(slot: number): void;
  setInventoryTab(tab: LocalInventoryTab): void;
  swapSlots(from: number, to: number): void;
  onRestart(): void;
}

/** All Blocks 页：可采集资源 + 固定工具图鉴 */
const BLOCK_CATALOG: ReadonlyArray<{
  id: string;
  label: string;
  icon: LocalResourceKey | "pickaxe" | "sword" | "hand";
}> = [
  ...LOCAL_RESOURCE_KEYS.map((id) => ({
    id,
    label: LOCAL_RESOURCE_LABELS[id],
    icon: id,
  })),
  { id: "pickaxe", label: "铁镐", icon: "pickaxe" },
  { id: "sword", label: "铁剑", icon: "sword" },
  { id: "hand", label: "空手", icon: "hand" },
];

export class LocalGameView {
  readonly canvas: HTMLCanvasElement;
  private readonly shell: HTMLElement;
  private readonly elapsed: HTMLElement;
  private readonly styleLabel: HTMLElement;
  private readonly target: HTMLElement;
  private readonly extraction: HTMLElement;
  private readonly extractionFill: HTMLElement;
  private readonly hint: HTMLElement;
  private readonly loading: HTMLElement;
  private readonly notice: HTMLElement;
  private readonly result: HTMLElement;
  private readonly resultTime: HTMLElement;
  private readonly resultList: HTMLElement;
  private readonly healthBar: HTMLElement;
  private readonly hotbarSlots: HTMLElement[];
  private readonly panelSlots: HTMLElement[];
  private readonly inventoryPanel: HTMLElement;
  private readonly inventoryTabItems: HTMLButtonElement;
  private readonly inventoryTabBlocks: HTMLButtonElement;
  private readonly inventoryClose: HTMLButtonElement;
  private readonly inventoryPaneItems: HTMLElement;
  private readonly inventoryPaneBlocks: HTMLElement;
  private readonly restartButton: HTMLButtonElement;
  private readonly dragGhost: HTMLElement;
  private readonly drag: InventoryDragController;
  private latestInventory: LocalGameState["inventory"] | null = null;

  constructor(
    private readonly root: HTMLElement,
    private readonly actions: LocalViewActions,
  ) {
    root.innerHTML = template();
    this.shell = required(root, ".single-shell");
    this.canvas = required(root, ".single-canvas");
    this.elapsed = required(root, ".single-elapsed");
    this.styleLabel = required(root, ".single-style");
    this.target = required(root, ".single-target");
    this.extraction = required(root, ".single-extraction-progress");
    this.extractionFill = required(root, ".single-extraction-fill");
    this.hint = required(root, ".single-hint");
    this.loading = required(root, ".single-loading");
    this.notice = required(root, ".single-notice");
    this.result = required(root, ".single-result");
    this.resultTime = required(root, ".single-result-time");
    this.resultList = required(root, ".single-result-resources");
    this.healthBar = required(root, ".single-health");
    this.hotbarSlots = Array.from(
      root.querySelectorAll<HTMLElement>(".single-hotbar .single-slot"),
    );
    this.panelSlots = Array.from(
      root.querySelectorAll<HTMLElement>(".single-inv-items .single-slot"),
    );
    this.inventoryPanel = required(root, ".single-inventory");
    this.inventoryTabItems = required(root, '[data-inv-tab="items"]');
    this.inventoryTabBlocks = required(root, '[data-inv-tab="blocks"]');
    this.inventoryClose = required(root, ".single-inventory-close");
    this.inventoryPaneItems = required(root, ".single-inv-items");
    this.inventoryPaneBlocks = required(root, ".single-inv-blocks");
    this.restartButton = required(root, ".single-restart");
    this.dragGhost = required(root, ".single-drag-ghost");
    this.drag = new InventoryDragController(
      this.shell,
      this.dragGhost,
      () => [...this.hotbarSlots, ...this.panelSlots],
      (index) => this.latestInventory?.[index] ?? null,
      this.actions.selectSlot,
      this.actions.swapSlots,
    );

    this.restartButton.addEventListener("click", this.actions.onRestart);
    this.inventoryClose.addEventListener("click", this.actions.closeInventory);
    this.inventoryTabItems.addEventListener("click", () =>
      this.actions.setInventoryTab("items"),
    );
    this.inventoryTabBlocks.addEventListener("click", () =>
      this.actions.setInventoryTab("blocks"),
    );
    this.inventoryPanel.addEventListener("pointerdown", this.handleBackdrop);
  }

  render(state: LocalGameState, frame: LocalViewFrame): void {
    this.latestInventory = state.inventory;
    this.shell.dataset.phase = state.phase;
    this.shell.dataset.inventory = state.inventoryOpen ? "open" : "closed";
    this.elapsed.textContent = formatElapsed(state.elapsedMs);
    const styleText = frame.styleLabel?.trim() ?? "";
    this.styleLabel.hidden = styleText === "" || state.phase === "loading";
    this.styleLabel.textContent = styleText;
    this.loading.hidden = state.phase !== "loading";
    this.target.hidden =
      frame.targetName === null ||
      state.phase !== "playing" ||
      state.inventoryOpen;
    this.target.textContent = frame.targetName ?? "";
    this.renderHint(state);
    this.renderExtraction(state, frame.insideExtraction);
    this.renderHealth(state);
    this.renderHotbar(state);
    this.renderInventoryPanel(state);
    this.notice.hidden = state.notice === null;
    this.notice.textContent = state.notice ?? "";
    this.renderResult(state);
  }

  dispose(): void {
    this.drag.dispose();
    this.restartButton.removeEventListener("click", this.actions.onRestart);
    this.inventoryClose.removeEventListener(
      "click",
      this.actions.closeInventory,
    );
    this.inventoryPanel.removeEventListener("pointerdown", this.handleBackdrop);
    this.root.replaceChildren();
  }

  private readonly handleBackdrop = (event: PointerEvent): void => {
    if (event.target === this.inventoryPanel) this.actions.closeInventory();
  };

  private renderHint(state: LocalGameState): void {
    this.hint.hidden =
      state.hint === "hidden" ||
      state.phase !== "playing" ||
      state.inventoryOpen;
    this.hint.dataset.mode = state.hint;
    this.hint.innerHTML =
      state.hint === "resume"
        ? "<strong>点击继续</strong>"
        : [
            "<strong>进入矿坑</strong>",
            "<span>WASD 移动　空格跳跃　左键挖/攻击　右键放置</span>",
            "<span>1 空手(半心)　2 铁镐　3 铁剑(一心)　Q 丢弃　E 背包</span>",
          ].join("");
  }

  private renderHealth(state: LocalGameState): void {
    const show = state.phase === "playing" || state.phase === "extracted";
    this.healthBar.hidden = !show;
    if (!show) return;
    const icons = heartsFromHealth(state.playerHealth);
    // 仅在心数变化时重绘，避免每帧改 DOM
    const signature = icons.join(",");
    if (this.healthBar.dataset.sig === signature) return;
    this.healthBar.dataset.sig = signature;
    this.healthBar.innerHTML = icons.map((icon) => heartMarkup(icon)).join("");
    this.healthBar.setAttribute(
      "aria-label",
      `生命 ${(state.playerHealth / 2).toFixed(state.playerHealth % 2 === 0 ? 0 : 1)} / 10`,
    );
  }

  private renderExtraction(state: LocalGameState, inside: boolean): void {
    this.extraction.hidden =
      !inside || state.phase !== "playing" || state.inventoryOpen;
    this.extractionFill.style.width = `${Math.min(1, state.extractionElapsedMs / LOCAL_EXTRACTION_REQUIRED_MS) * 100}%`;
  }

  private renderHotbar(state: LocalGameState): void {
    paintSlots(this.hotbarSlots, state);
  }

  private renderInventoryPanel(state: LocalGameState): void {
    const open = state.phase === "playing" && state.inventoryOpen;
    this.inventoryPanel.hidden = !open;
    this.inventoryPanel.setAttribute("aria-hidden", String(!open));
    if (!open) return;

    this.inventoryTabItems.dataset.active = String(
      state.inventoryTab === "items",
    );
    this.inventoryTabBlocks.dataset.active = String(
      state.inventoryTab === "blocks",
    );
    this.inventoryPaneItems.hidden = state.inventoryTab !== "items";
    this.inventoryPaneBlocks.hidden = state.inventoryTab !== "blocks";
    paintSlots(this.panelSlots, state);
  }

  private renderResult(state: LocalGameState): void {
    const result = state.result;
    this.result.hidden = state.phase !== "extracted" || result === null;
    if (result === null) return;
    this.resultTime.textContent = formatElapsed(result.elapsedMs);
    // 只列出本局拿到过的种类；全空则提示空包撤离
    const rows = LOCAL_RESOURCE_KEYS.filter(
      (key) => (result.resources[key] ?? 0) > 0,
    );
    if (rows.length === 0) {
      this.resultList.innerHTML =
        '<div class="single-result-empty">空背包撤离 · 未采集资源</div>';
      return;
    }
    this.resultList.innerHTML = rows
      .map(
        (key) => `
      <div class="single-result-row" data-resource="${key}">
        <i class="single-slot-icon" data-icon="${key}" aria-hidden="true"></i>
        <dt>${LOCAL_RESOURCE_LABELS[key]}</dt>
        <dd>${result.resources[key]}</dd>
      </div>`,
      )
      .join("");
  }
}

function paintSlots(elements: HTMLElement[], state: LocalGameState): void {
  elements.forEach((element, index) => {
    const slot = state.inventory[index] ?? null;
    element.dataset.selected = String(index === state.selectedSlot);
    // 快捷栏 1/2/3 键对应槽 0/1/2：空手、镐、剑（固定工具，叠在资源槽上显示）
    const tool = toolForHotbarIndex(index);
    if (tool !== null) {
      element.dataset.tool = tool;
      element.dataset.resource = "empty";
    } else {
      delete element.dataset.tool;
      element.dataset.resource = slot?.resource ?? "empty";
    }
    const quantity = element.querySelector<HTMLElement>(
      ".single-slot-quantity",
    );
    if (quantity !== null) {
      // 工具槽不显示资源数量，避免盖住镐/剑图标
      const show = tool === null && slot !== null && slot.quantity > 0;
      quantity.hidden = !show;
      quantity.textContent = show ? String(slot.quantity) : "";
    }
  });
}

/** 槽 0 空手，1 镐，2 剑；其余为资源（与 heldToolFromSlot 对齐） */
function toolForHotbarIndex(
  index: number,
): "hand" | "pickaxe" | "sword" | null {
  if (index === 0) return "hand";
  if (index === 1) return "pickaxe";
  if (index === 2) return "sword";
  return null;
}

export function formatElapsed(elapsedMs: number): string {
  const totalSeconds = Math.max(0, Math.floor(elapsedMs / 1_000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function template(): string {
  const hotbarSlots = slotRow("hotbar", LOCAL_INVENTORY_SLOTS);
  const panelSlots = slotRow("panel", LOCAL_INVENTORY_SLOTS);
  const catalog = BLOCK_CATALOG.map(
    (block) => `
    <span class="single-catalog-slot" data-icon="${block.icon}" title="${block.label}">
      <i class="single-slot-icon" data-icon="${block.icon}" aria-hidden="true"></i>
      <em>${block.label}</em>
    </span>`,
  ).join("");

  return `
    <main class="single-shell" data-phase="loading" data-inventory="closed">
      <canvas class="single-canvas" aria-label="单机体素采石场"></canvas>
      <div class="single-vignette" aria-hidden="true"></div>
      <div class="single-local-badge" role="status">本地单机 · 不保存进度</div>
      <div class="single-elapsed" aria-label="本局经过时间">00:00</div>
      <div class="single-style" hidden aria-label="地图风格"></div>
      <div class="single-target" hidden></div>
      <div class="single-crosshair" aria-hidden="true"><span></span><span></span></div>
      <div class="single-extraction-progress" hidden>
        <span>保持停留 · 撤离</span><i><b class="single-extraction-fill"></b></i>
      </div>
      <div class="single-health" hidden aria-label="生命值"></div>
      <div class="single-hotbar" aria-label="快捷栏">
        ${hotbarSlots}
      </div>
      <div class="single-hint" data-mode="controls"></div>
      <div class="single-loading"><i></i><span>正在塑造废弃采石场</span></div>
      <div class="single-notice" role="status" aria-live="polite" hidden></div>

      <div class="single-inventory" hidden aria-hidden="true" role="dialog" aria-modal="true" aria-labelledby="single-inv-title">
        <div class="single-inventory-window">
          <header class="single-inventory-header">
            <h2 id="single-inv-title">Inventory</h2>
            <button type="button" class="single-inventory-close" aria-label="关闭">×</button>
          </header>
          <div class="single-inventory-tabs" role="tablist">
            <button type="button" class="single-inventory-tab" data-inv-tab="items" data-active="true" role="tab">My Items</button>
            <button type="button" class="single-inventory-tab" data-inv-tab="blocks" data-active="false" role="tab">All Blocks</button>
          </div>
          <div class="single-inv-items" role="tabpanel">
            <div class="single-inv-grid">
              ${panelSlots}
            </div>
            <p class="single-inv-note">1 空手 · 2 铁镐 · 3 铁剑 · 拖拽整理 · Q 丢弃</p>
          </div>
          <div class="single-inv-blocks" role="tabpanel" hidden>
            <div class="single-catalog-grid">
              ${catalog}
            </div>
            <p class="single-inv-note">图鉴只读 · 采集后进入 My Items</p>
          </div>
        </div>
      </div>

      <section class="single-result" hidden aria-labelledby="single-result-title">
        <span class="single-result-kicker">EXTRACTION COMPLETE</span>
        <h1 id="single-result-title">已撤离</h1>
        <p>用时 <strong class="single-result-time">00:00</strong></p>
        <dl class="single-result-resources"></dl>
        <small>本地单机记录，不保存</small>
        <button class="single-restart" type="button">再次进入</button>
      </section>
      <div class="single-drag-ghost" data-resource="empty" hidden aria-hidden="true"></div>
    </main>`;
}

function slotRow(scope: string, count: number): string {
  return Array.from(
    { length: count },
    (_, index) => `
    <span class="single-slot" data-scope="${scope}" data-resource="empty" data-selected="${index === 0}" data-index="${index}" aria-label="槽位 ${slotKey(index)}">
      <i class="single-slot-icon" aria-hidden="true"></i>
      <strong class="single-slot-quantity" hidden></strong>
    </span>`,
  ).join("");
}

function required<T extends Element>(root: ParentNode, selector: string): T {
  const element = root.querySelector<T>(selector);
  if (element === null) throw new Error(`单机界面缺少元素 ${selector}`);
  return element;
}

function slotKey(index: number): string {
  if (index < 9) return String(index + 1);
  return index === 9 ? "0" : index === 10 ? "-" : "+";
}

function heartMarkup(icon: HeartIcon): string {
  return `<i class="single-heart" data-heart="${icon}" aria-hidden="true"></i>`;
}
