import { Vector3 } from "three";

import { LocalSfx } from "./audio";
import { LocalBlockDurability } from "./block-durability";
import { LOCAL_BLOCK_DISPLAY_NAMES } from "./blocks";
import { fullHealth, isDead } from "./combat";
import {
  hasFallenOutOfTerrain,
  isInsideExtraction,
  miningProgress,
  pickRandomSkyDrop,
} from "./gameplay-math";
import { heldContentFromSlot } from "./held-content";
import { LocalInputController, cycleInventorySlot } from "./input";
import { LocalLootSystem } from "./loot";
import {
  formatMapStyleLabel,
  LOCAL_MAX_HEIGHT,
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
} from "./map";
import {
  tickMannequinRespawn,
  tryMannequinAttack,
} from "./mannequin-session";
import {
  advanceMiningSession,
  syncBreakCrackEntries,
} from "./mining-session";
import { attemptPlace, placeVoxelKey } from "./place-action";
import { LocalWorldRuntime, type LocalRuntimeFrame } from "./runtime";
import {
  addResource,
  createInitialLocalGameState,
  dropInventorySlot,
  LOCAL_EXTRACTION_REQUIRED_MS,
  reduceLocalGameState,
  type LocalGameAction,
  type LocalResourceKey,
} from "./state";
import { LocalGameView } from "./view";

export class SinglePlayerController {
  private readonly view: LocalGameView;
  private readonly sfx = new LocalSfx();
  private state = createInitialLocalGameState();
  private runtime: LocalWorldRuntime | null = null;
  private input: LocalInputController | null = null;
  private loot: LocalLootSystem | null = null;
  /** 世界方块耐久：松开/换人续挖不重置 */
  private readonly durability = new LocalBlockDurability();
  private animationFrame = 0;
  /** rAF 被后台标签/自动化节流时的备份驱动（就绪后可停） */
  private backupTimer = 0;
  private sessionGeneration = 0;
  private targetName: string | null = null;
  private insideExtraction = false;
  private claimed = new Set<string>();
  private noticeTimeout = 0;
  private disposed = false;
  private lastDigAt = 0;
  private lastExtractBucket = -1;
  private wasExtracted = false;
  /** 假人近战会话（生命 / 复活 / 冷却） */
  private mannequin = {
    health: fullHealth(),
    respawnAt: 0,
    deathXZ: null as [number, number] | null,
    attackCooldownUntil: 0,
  };
  private wasPrimaryHeld = false;
  private wasSecondaryHeld = false;
  /** 右键连放冷却（对齐 demo 连点节奏） */
  private placeCooldownUntil = 0;
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

  /**
   * 浏览器调试用：注入资源、选槽、检查第一人称手持场景树。
   * 挂到 window.__singleDebug（见 main.ts）。
   */
  debugGive(resource: LocalResourceKey, quantity = 8): void {
    const result = addResource(this.state.inventory, resource, quantity);
    this.dispatch({ type: "INVENTORY_REPLACED", inventory: result.inventory });
    // 自动选中第一个该资源槽
    const idx = result.inventory.findIndex(
      (s) => s?.resource === resource && s.quantity > 0,
    );
    if (idx >= 0) {
      this.dispatch({ type: "SLOT_SELECTED", slot: idx });
    }
    this.syncHeldTool(false);
  }

  debugSelectSlot(slot: number): void {
    this.dispatch({ type: "SLOT_SELECTED", slot });
    this.syncHeldTool(false);
  }

  debugHeldSnapshot(): {
    selectedSlot: number;
    content: ReturnType<typeof heldContentFromSlot>;
    arm: ReturnType<LocalWorldRuntime["debugArmHeld"]> | null;
  } {
    return {
      selectedSlot: this.state.selectedSlot,
      content: heldContentFromSlot(
        this.state.selectedSlot,
        this.state.inventory,
      ),
      arm: this.runtime?.debugArmHeld() ?? null,
    };
  }

  start(): void {
    if (this.animationFrame !== 0 || this.backupTimer !== 0) return;
    void this.createSession();
    this.animationFrame = requestAnimationFrame(this.animate);
    // 后台页/自动化环境 rAF 可能几乎不跑，导致永远 loading、热栏被藏
    this.backupTimer = window.setInterval(() => {
      this.tickFrame(performance.now());
    }, 50);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    cancelAnimationFrame(this.animationFrame);
    this.animationFrame = 0;
    window.clearInterval(this.backupTimer);
    this.backupTimer = 0;
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
    // 先挂下一帧，避免 update 抛错时 rAF 链永久断开
    if (!this.disposed) {
      this.animationFrame = requestAnimationFrame(this.animate);
    }
    this.tickFrame(now);
  };

  private tickFrame(now: number): void {
    if (this.disposed) return;
    const runtime = this.runtime;
    if (runtime === null) return;
    try {
      this.handleFrame(runtime, runtime.update(now), now);
    } catch (error) {
      console.error("[single] 帧循环异常", error);
      this.dispatch({
        type: "FAILED",
        message: "本地世界更新失败，请刷新重试",
      });
    }
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
        this.tickMannequinRespawn(now);
        // 右键放置与左键挖掘/攻击并行（不同鼠标键）
        this.tryPlace(runtime, frame, now);
        const attacked = this.tryAttack(runtime, now);
        if (!attacked) {
          this.advanceMining(runtime, frame, now);
        } else {
          this.stopActiveMining();
        }
        this.playStepSfx(frame, now);
      } else {
        this.input?.cancelMining();
        this.input?.cancelPlace();
        this.stopActiveMining();
      }
    }
    this.wasPrimaryHeld = this.input?.primaryHeld === true;
    this.wasSecondaryHeld = this.input?.secondaryHeld === true;
    // 挥臂仅在主动按住挖掘时；裂纹按世界耐久显示（松开也保留）
    const digProgress = miningProgress(this.state);
    runtime.setMiningProgress(digProgress);
    this.syncBreakCrack(runtime);
    this.render();
  }

  /**
   * 右键放置：委托 place-action（邻格 / 扣 1 / 冷却）。
   */
  private tryPlace(
    runtime: LocalWorldRuntime,
    frame: LocalRuntimeFrame,
    now: number,
  ): void {
    const result = attemptPlace({
      pointerLocked: runtime.isLocked,
      secondaryHeld: this.input?.secondaryHeld === true,
      wasSecondaryHeld: this.wasSecondaryHeld,
      now,
      placeCooldownUntil: this.placeCooldownUntil,
      selectedSlot: this.state.selectedSlot,
      inventory: this.state.inventory,
      potential: frame.potential,
      canPlace: (voxel, blockId) => runtime.canPlaceBlock(voxel, blockId),
      place: (voxel, blockId) => runtime.placeBlock(voxel, blockId),
    });
    if (result.kind !== "placed") return;
    this.dispatch({
      type: "INVENTORY_REPLACED",
      inventory: result.inventory,
    });
    const key = placeVoxelKey(result.voxel);
    this.durability.clear(key);
    this.claimed.delete(key);
    this.syncHeldTool(false);
    runtime.playAttackSwing();
    this.sfx.play("dig", { rate: 1.08 });
    this.placeCooldownUntil = result.nextCooldownUntil;
  }

  /** 近战：准星命中假人时优先攻击（不挖方块）。 */
  private tryAttack(runtime: LocalWorldRuntime, now: number): boolean {
    const result = tryMannequinAttack(runtime, {
      pointerLocked: runtime.isLocked,
      primaryHeld: this.input?.primaryHeld === true,
      wasPrimaryHeld: this.wasPrimaryHeld,
      selectedSlot: this.state.selectedSlot,
      now,
      state: this.mannequin,
    });
    if (result.kind === "miss") return false;
    if (result.kind === "aiming") return true;
    this.mannequin = result.state;
    if (result.targetName !== null) this.targetName = result.targetName;
    if (result.notice !== null) this.showNotice(result.notice);
    this.sfx.play("dig", { rate: result.sfxRate });
    if (result.killDrop !== null) {
      this.loot?.dropTool(
        result.killDrop.tool,
        result.killDrop.position,
        now,
        350,
      );
      this.sfx.play("drop");
    }
    return true;
  }

  private tickMannequinRespawn(now: number): void {
    const next = tickMannequinRespawn(this.runtime, this.mannequin, now);
    this.mannequin = next.state;
    if (next.notice !== null) this.showNotice(next.notice);
  }

  private collectLoot(position: Vector3, now: number, deltaMs: number): void {
    let inventoryChanged = false;
    this.loot?.updateAndCollect(position, now, deltaMs, (pickup) => {
      if (pickup.kind === "tool") {
        // 工具快捷栏已常驻；拾取仅反馈，不占资源格
        this.showNotice(pickup.tool === "sword" ? "捡到铁剑" : "捡到铁镐");
        this.sfx.play("pickup");
        return 0;
      }
      const result = addResource(
        this.state.inventory,
        pickup.resource,
        pickup.quantity,
      );
      this.dispatch({
        type: "INVENTORY_REPLACED",
        inventory: result.inventory,
      });
      if (result.remainder < pickup.quantity) {
        this.sfx.play("pickup");
        inventoryChanged = true;
      }
      return result.remainder;
    });
    // 选中资源槽从空→有时需刷新手持方块
    if (inventoryChanged) this.syncHeldTool(false);
  }

  private advanceMining(
    runtime: LocalWorldRuntime,
    frame: LocalRuntimeFrame,
    now: number,
  ): void {
    this.lastDigAt = advanceMiningSession(
      {
        selectedSlot: this.state.selectedSlot,
        mining: this.state.mining,
        claimed: this.claimed,
        durability: this.durability,
        primaryHeld: this.input?.primaryHeld === true,
        pointerLocked: runtime.isLocked,
        lastDigAt: this.lastDigAt,
        dispatch: (action) => this.dispatch(action),
        stopActiveMining: () => this.stopActiveMining(),
        playSfx: (name, opts) => this.sfx.play(name, opts),
        dropLoot: (resource, quantity, position, t, excludeMs) => {
          this.loot?.drop(resource, quantity, position, t, excludeMs);
        },
      },
      runtime,
      frame,
      now,
    );
  }

  private syncBreakCrack(runtime: LocalWorldRuntime): void {
    syncBreakCrackEntries(
      { claimed: this.claimed, durability: this.durability },
      runtime,
    );
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
    this.syncHeldTool(false);
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
    this.runtime?.setHeldContent(
      heldContentFromSlot(this.state.selectedSlot, this.state.inventory),
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
    // 虚空坠落扣 1 星；致死则满血重生
    this.dispatch({ type: "PLAYER_DAMAGED", amount: 2 });
    if (isDead(this.state.playerHealth)) {
      this.dispatch({ type: "PLAYER_HEALED" });
      this.showNotice("你死了 · 满血重降");
    } else {
      this.showNotice("坠落出界 · 随机高空重降");
    }
    runtime.respawnRandomFromSky(drop);
    // 同步 frame 位置，避免本帧仍用旧坐标做撤离/拾取判定
    frame.playerPosition.set(drop[0], drop[1], drop[2]);
    this.lastVoidRespawnAt = now;
    this.input?.cancelMining();
    this.cancelMining();
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
    // 撤离倒计时内约每秒一响
    const bucket = Math.floor(elapsed / 1_000);
    if (
      bucket !== this.lastExtractBucket &&
      elapsed < LOCAL_EXTRACTION_REQUIRED_MS
    ) {
      this.lastExtractBucket = bucket;
      if (prevElapsed > 0 || bucket === 0) this.sfx.play("extractTick");
    }
  }

  /** 结束当前挥挖会话（不清除世界方块耐久） */
  private stopActiveMining(): void {
    if (this.state.mining !== null) this.dispatch({ type: "MINING_CANCELLED" });
  }

  /** @deprecated 兼容旧调用名 → stopActiveMining */
  private cancelMining(): void {
    this.stopActiveMining();
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
    this.durability.clearAll();
    this.mannequin = {
      health: fullHealth(),
      respawnAt: 0,
      deathXZ: null,
      attackCooldownUntil: 0,
    };
    this.wasPrimaryHeld = false;
    this.wasSecondaryHeld = false;
    this.placeCooldownUntil = 0;
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
      styleLabel: this.runtime
        ? formatMapStyleLabel(this.runtime.adapter.map)
        : null,
    });
  }
}
