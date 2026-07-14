import { Vector3 } from "three";

import { LocalSfx } from "./audio";
import {
  getBlockMiningProfile,
  LOCAL_BLOCK_DEBRIS_COLORS,
  LOCAL_BLOCK_DISPLAY_NAMES,
  LOCAL_BLOCK_IDS,
} from "./blocks";
import {
  hasFallenOutOfTerrain,
  isInsideExtraction,
  miningProgress,
  pickRandomSkyDrop,
} from "./gameplay-math";
import { LocalInputController, cycleInventorySlot } from "./input";
import { LocalLootSystem } from "./loot";
import {
  LOCAL_MAX_HEIGHT,
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
} from "./map";
import {
  digIntervalMs,
  digRateFor,
  harvestDrop,
  miningDurationMs,
} from "./mining";
import { LocalWorldRuntime, type LocalRuntimeFrame } from "./runtime";
import {
  addResource,
  createInitialLocalGameState,
  dropInventorySlot,
  reduceLocalGameState,
  voxelKey,
  type LocalGameAction,
  type LocalResourceKey,
} from "./state";
import { LocalGameView } from "./view";
import { heldToolFromSlot } from "./viewmodel";

export class SinglePlayerController {
  private readonly view: LocalGameView;
  private readonly sfx = new LocalSfx();
  private state = createInitialLocalGameState();
  private runtime: LocalWorldRuntime | null = null;
  private input: LocalInputController | null = null;
  private loot: LocalLootSystem | null = null;
  private animationFrame = 0;
  private sessionGeneration = 0;
  private targetName: string | null = null;
  private insideExtraction = false;
  private claimed = new Set<string>();
  private noticeTimeout = 0;
  private disposed = false;
  private lastDigAt = 0;
  private lastExtractBucket = -1;
  private wasExtracted = false;
  /** 虚空重生冷却，避免同帧连跳 */
  private lastVoidRespawnAt = 0;

  constructor(root: HTMLElement) {
    this.view = new LocalGameView(root, {
      closeInventory: () => {
        this.sfx.unlock();
        this.dispatch({ type: "CLOSE_INVENTORY" });
        this.sfx.play("inventoryClose");
      },
      selectSlot: (slot) => {
        this.sfx.unlock();
        if (slot !== this.state.selectedSlot) {
          this.sfx.play("select");
          // 换工具会改变硬度结算，取消进行中的挖掘
          this.cancelMining();
        }
        this.dispatch({ type: "SLOT_SELECTED", slot });
        this.syncHeldTool(true);
      },
      setInventoryTab: (tab) => {
        this.sfx.unlock();
        this.sfx.play("ui");
        this.dispatch({ type: "SET_INVENTORY_TAB", tab });
      },
      swapSlots: (from, to) => {
        this.sfx.unlock();
        this.sfx.play("ui");
        this.dispatch({ type: "INVENTORY_SWAP", from, to });
        // 交换后 selectedSlot 可能变化，同步第一人称握持
        this.syncHeldTool(true);
      },
      onRestart: () => void this.restart(),
    });
    this.view.render(this.state, { insideExtraction: false, targetName: null });
    // 任意指针/键盘手势后解锁音频上下文
    window.addEventListener("pointerdown", this.unlockAudio, { once: true });
    window.addEventListener("keydown", this.unlockAudio, { once: true });
  }

  start(): void {
    if (this.animationFrame !== 0) return;
    void this.createSession();
    this.animationFrame = requestAnimationFrame(this.animate);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    cancelAnimationFrame(this.animationFrame);
    window.clearTimeout(this.noticeTimeout);
    window.removeEventListener("pointerdown", this.unlockAudio);
    window.removeEventListener("keydown", this.unlockAudio);
    this.teardownSession();
    this.sfx.dispose();
    this.view.dispose();
  }

  private readonly unlockAudio = (): void => {
    this.sfx.unlock();
  };

  private async createSession(): Promise<void> {
    const generation = ++this.sessionGeneration;
    const runtime = new LocalWorldRuntime(this.view.canvas, {
      onError: (message) => this.dispatch({ type: "FAILED", message }),
      onPointerLock: (locked) => {
        this.sfx.unlock();
        this.input?.cancelMining();
        this.dispatch({ type: locked ? "POINTER_LOCKED" : "POINTER_UNLOCKED" });
        if (locked) this.sfx.play("ui");
      },
      onWorldReady: () => this.dispatch({ type: "WORLD_READY" }),
    });
    const input = new LocalInputController(this.view.canvas, {
      closeInventory: () => {
        this.dispatch({ type: "CLOSE_INVENTORY" });
        this.sfx.play("inventoryClose");
      },
      cycleSlot: (direction) => {
        this.sfx.play("select");
        this.cancelMining();
        this.dispatch({
          type: "SLOT_SELECTED",
          slot: cycleInventorySlot(this.state.selectedSlot, direction),
        });
        this.syncHeldTool(true);
      },
      dropSelectedSlot: () => this.dropSelectedSlot(),
      isGameplayActive: () => this.state.phase === "playing",
      isInventoryOpen: () => this.state.inventoryOpen,
      isPointerLocked: () => this.runtime?.isLocked === true,
      selectSlot: (slot) => {
        if (slot !== this.state.selectedSlot) {
          this.sfx.play("select");
          this.cancelMining();
        }
        this.dispatch({ type: "SLOT_SELECTED", slot });
        this.syncHeldTool(true);
      },
      toggleHelp: () => {
        this.sfx.play("ui");
        this.dispatch({ type: "TOGGLE_HELP" });
      },
      toggleInventory: () => this.toggleInventory(),
    });
    const loot = new LocalLootSystem(runtime.world);
    this.runtime = runtime;
    this.input = input;
    this.loot = loot;
    await runtime.initialize();
    if (generation !== this.sessionGeneration || this.disposed) {
      runtime.dispose();
      return;
    }
    // 默认槽 0 = 空手手臂；镐/剑贴图异步预载
    this.syncHeldTool(false);
  }

  private readonly animate = (now: number): void => {
    if (this.disposed) return;
    const runtime = this.runtime;
    if (runtime !== null) this.handleFrame(runtime, runtime.update(now), now);
    this.animationFrame = requestAnimationFrame(this.animate);
  };

  private handleFrame(
    runtime: LocalWorldRuntime,
    frame: LocalRuntimeFrame,
    now: number,
  ): void {
    this.targetName =
      frame.target === null
        ? null
        : LOCAL_BLOCK_DISPLAY_NAMES[frame.target.id] ?? null;
    if (this.state.phase === "playing" && frame.ready) {
      this.maybeRespawnFromVoid(runtime, frame, now);
      this.insideExtraction = isInsideExtraction(
        frame.playerPosition,
        runtime.adapter.map.extraction,
      );
      const prevExtraction = this.state.extractionElapsedMs;
      this.dispatch({
        type: "FRAME",
        deltaMs: frame.deltaMs,
        insideExtraction: this.insideExtraction,
      });
      this.playExtractionSfx(prevExtraction);
      if (this.state.result !== null) {
        this.input?.cancelMining();
        runtime.freeze();
        if (!this.wasExtracted) {
          this.wasExtracted = true;
          this.sfx.play("extractDone");
        }
      } else if (!this.state.inventoryOpen) {
        this.collectLoot(frame.playerPosition, now, frame.deltaMs);
        this.advanceMining(runtime, frame, now);
        this.playStepSfx(frame, now);
      } else {
        this.input?.cancelMining();
        this.cancelMining();
      }
    }
    const digProgress = miningProgress(this.state);
    runtime.setMiningProgress(digProgress);
    runtime.setBreakCrack(digProgress, this.state.mining?.target ?? null);
    this.render();
  }

  private advanceMining(
    runtime: LocalWorldRuntime,
    frame: LocalRuntimeFrame,
    now: number,
  ): void {
    const tool = heldToolFromSlot(this.state.selectedSlot);
    const target = frame.target;
    const profile = target === null ? null : getBlockMiningProfile(target.id);
    const key = target === null ? null : voxelKey(target.voxel);
    if (
      this.input?.primaryHeld !== true ||
      !runtime.isLocked ||
      target === null ||
      profile === null ||
      key === null ||
      this.claimed.has(key)
    ) {
      this.cancelMining();
      return;
    }

    const requiredMs = miningDurationMs(profile, tool);
    const drop = harvestDrop(profile.drop, tool, profile);
    if (
      this.state.mining?.targetKey !== key ||
      this.state.mining.requiredMs !== requiredMs
    ) {
      this.dispatch({
        type: "MINING_STARTED",
        target: target.voxel,
        resource: drop,
        displayName: profile.displayName,
        requiredMs,
      });
      this.sfx.play("dig", { rate: digRateFor(drop, tool) });
      this.lastDigAt = now;
    }
    this.dispatch({
      type: "MINING_ADVANCED",
      deltaMs: frame.deltaMs,
      targetKey: key,
    });
    if (now - this.lastDigAt > digIntervalMs(tool)) {
      this.sfx.play("dig", { rate: digRateFor(drop, tool) });
      this.lastDigAt = now;
    }
    if (
      this.state.mining !== null &&
      this.state.mining.elapsedMs >= this.state.mining.requiredMs
    ) {
      this.completeMining(runtime, target, drop, now);
    }
  }

  private completeMining(
    runtime: LocalWorldRuntime,
    target: { id: number; voxel: [number, number, number] },
    drop: LocalResourceKey | null,
    now: number,
  ): void {
    const key = voxelKey(target.voxel);
    if (
      this.claimed.has(key) ||
      runtime.world.getVoxelAt(...target.voxel) !== target.id
    ) {
      this.cancelMining();
      return;
    }
    this.claimed.add(key);
    try {
      runtime.adapter.applyServerVoxelUpdate(
        runtime.world,
        target.voxel,
        LOCAL_BLOCK_IDS.air,
      );
    } catch {
      this.claimed.delete(key);
      this.cancelMining();
      return;
    }
    runtime.setBreakCrack(null, null);
    runtime.playBlockBreakBurst(
      target.voxel,
      LOCAL_BLOCK_DEBRIS_COLORS[target.id] ?? "#888888",
      now,
    );
    this.sfx.play("break", {
      rate: digRateFor(drop, heldToolFromSlot(this.state.selectedSlot)),
    });
    if (drop !== null) {
      this.loot?.drop(
        drop,
        1,
        [target.voxel[0] + 0.5, target.voxel[1] + 0.55, target.voxel[2] + 0.5],
        now,
        280,
      );
      this.sfx.play("drop");
    }
    this.cancelMining();
  }

  private collectLoot(position: Vector3, now: number, deltaMs: number): void {
    this.loot?.updateAndCollect(position, now, deltaMs, (resource, quantity) => {
      const result = addResource(this.state.inventory, resource, quantity);
      this.dispatch({ type: "INVENTORY_REPLACED", inventory: result.inventory });
      if (result.remainder < quantity) this.sfx.play("pickup");
      return result.remainder;
    });
  }

  private dropSelectedSlot(): void {
    const runtime = this.runtime;
    if (runtime === null || this.state.phase !== "playing") return;
    const result = dropInventorySlot(
      this.state.inventory,
      this.state.selectedSlot,
    );
    if (result.dropped === null) return;
    const position = runtime.camera.getWorldPosition(new Vector3());
    position.addScaledVector(runtime.getDirection(), 1.15);
    position.y -= 0.85;
    this.loot?.drop(
      result.dropped.resource,
      result.dropped.quantity,
      position.toArray(),
      performance.now(),
      900,
    );
    this.dispatch({ type: "INVENTORY_REPLACED", inventory: result.inventory });
    this.sfx.play("drop");
  }

  private toggleInventory(): void {
    if (this.state.phase !== "playing") return;
    this.sfx.unlock();
    const opening = !this.state.inventoryOpen;
    this.dispatch({ type: "TOGGLE_INVENTORY" });
    this.sfx.play(opening ? "inventoryOpen" : "inventoryClose");
    if (opening) {
      this.input?.cancelMining();
      this.runtime?.unlockPointer();
    }
  }

  private syncHeldTool(animate: boolean): void {
    this.runtime?.setHeldTool(
      heldToolFromSlot(this.state.selectedSlot),
      animate,
    );
  }

  private playStepSfx(frame: LocalRuntimeFrame, _now: number): void {
    // builder 原文：f.state.running && atRestY===-1 && !swimming && !busy
    // busy / 播完+间隔 在 LocalSfx 内，与 playAudio.onended 对齐
    if (!frame.ready || !frame.running || !frame.onGround) return;
    this.sfx.play("step", { sprinting: frame.sprinting });
  }

  /** 坠落出地形 → 地图内随机点高空落下 */
  private maybeRespawnFromVoid(
    runtime: LocalWorldRuntime,
    frame: LocalRuntimeFrame,
    now: number,
  ): void {
    if (this.state.result !== null || this.state.inventoryOpen) return;
    if (now - this.lastVoidRespawnAt < 400) return;
    if (
      !hasFallenOutOfTerrain(
        frame.playerPosition,
        LOCAL_WORLD_MIN,
        LOCAL_WORLD_MAX,
      )
    ) {
      return;
    }
    const drop = pickRandomSkyDrop(
      LOCAL_WORLD_MIN,
      LOCAL_WORLD_MAX,
      runtime.adapter.map.surfaceY,
      { maxY: LOCAL_MAX_HEIGHT - 2 },
    );
    runtime.respawnRandomFromSky(drop);
    // 同步 frame 位置，避免本帧仍用旧坐标做撤离/拾取判定
    frame.playerPosition.set(drop[0], drop[1], drop[2]);
    this.lastVoidRespawnAt = now;
    this.input?.cancelMining();
    this.cancelMining();
    this.showNotice("坠落出界 · 随机高空重降");
  }

  private playExtractionSfx(prevElapsed: number): void {
    if (!this.insideExtraction || this.state.inventoryOpen) {
      this.lastExtractBucket = -1;
      return;
    }
    const elapsed = this.state.extractionElapsedMs;
    if (elapsed <= 0) {
      this.lastExtractBucket = -1;
      return;
    }
    // 撤离 3 秒内约每秒一响
    const bucket = Math.floor(elapsed / 1_000);
    if (bucket !== this.lastExtractBucket && elapsed < 3_000) {
      this.lastExtractBucket = bucket;
      if (prevElapsed > 0 || bucket === 0) this.sfx.play("extractTick");
    }
  }

  private cancelMining(): void {
    if (this.state.mining !== null) this.dispatch({ type: "MINING_CANCELLED" });
  }

  private showNotice(message: string): void {
    window.clearTimeout(this.noticeTimeout);
    this.dispatch({ type: "NOTICE_SET", notice: message });
    this.noticeTimeout = window.setTimeout(
      () => this.dispatch({ type: "NOTICE_SET", notice: null }),
      1_800,
    );
  }

  private async restart(): Promise<void> {
    this.sfx.play("ui");
    this.teardownSession();
    this.state = createInitialLocalGameState();
    this.claimed = new Set();
    this.targetName = null;
    this.insideExtraction = false;
    this.lastExtractBucket = -1;
    this.wasExtracted = false;
    this.render();
    await this.createSession();
  }

  private teardownSession(): void {
    this.sessionGeneration += 1;
    this.input?.dispose();
    this.loot?.dispose();
    this.runtime?.dispose();
    this.input = null;
    this.loot = null;
    this.runtime = null;
  }

  private dispatch(action: LocalGameAction): void {
    this.state = reduceLocalGameState(this.state, action);
    this.render();
  }

  private render(): void {
    this.view.render(this.state, {
      insideExtraction: this.insideExtraction,
      targetName: this.targetName,
      styleLabel: this.runtime?.adapter.map.style.label ?? null,
    });
  }
}


