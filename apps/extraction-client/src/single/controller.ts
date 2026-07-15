import { Vector3 } from "three";

import { LocalSfx } from "./audio";
import {
  getBlockMiningProfile,
  LOCAL_BLOCK_DEBRIS_COLORS,
  LOCAL_BLOCK_DISPLAY_NAMES,
  LOCAL_BLOCK_IDS,
} from "./blocks";
import { LocalBlockDurability } from "./block-durability";
import {
  applyDamage,
  attackDamageHalfHearts,
  fullHealth,
  isDead,
  knockbackImpulse,
  LOCAL_ATTACK_COOLDOWN_MS,
  LOCAL_ATTACK_RANGE,
} from "./combat";
import {
  hasFallenOutOfTerrain,
  isInsideExtraction,
  miningProgress,
  pickRandomSkyDrop,
} from "./gameplay-math";
import { LocalInputController, cycleInventorySlot } from "./input";
import { LocalLootSystem } from "./loot";
import {
  formatMapStyleLabel,
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
  LOCAL_EXTRACTION_REQUIRED_MS,
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
  /** 世界方块耐久：松开/换人续挖不重置 */
  private readonly durability = new LocalBlockDurability();
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
  /** 假人生命（半心）；与玩家独立 */
  private mannequinHealth = fullHealth();
  private mannequinRespawnAt = 0;
  /** 击杀时 xz，复活落在附近 */
  private mannequinDeathXZ: [number, number] | null = null;
  private attackCooldownUntil = 0;
  private wasPrimaryHeld = false;
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
        this.tickMannequinRespawn(now);
        const attacked = this.tryAttack(runtime, now);
        if (!attacked) {
          this.advanceMining(runtime, frame, now);
        } else {
          this.stopActiveMining();
        }
        this.playStepSfx(frame, now);
      } else {
        this.input?.cancelMining();
        this.stopActiveMining();
      }
    }
    this.wasPrimaryHeld = this.input?.primaryHeld === true;
    // 挥臂仅在主动按住挖掘时；裂纹按世界耐久显示（松开也保留）
    const digProgress = miningProgress(this.state);
    runtime.setMiningProgress(digProgress);
    this.syncBreakCrack(runtime, frame);
    this.render();
  }

  /**
   * 近战：准星命中假人时优先攻击（不挖方块）。
   * 上升沿或冷却结束后按住可再砍；伤害见 combat.ts。
   */
  private tryAttack(runtime: LocalWorldRuntime, now: number): boolean {
    if (!runtime.isLocked || this.input?.primaryHeld !== true) return false;
    if (this.mannequinHealth <= 0) return false;

    const hitDist = runtime.raycastMannequin(LOCAL_ATTACK_RANGE);
    if (hitDist === null) return false;

    const rising = !this.wasPrimaryHeld;
    const cooled = now >= this.attackCooldownUntil;
    // 点按立刻砍；按住则等冷却结束再砍（类似原版连点节奏）
    if (!rising && !cooled) return true; // 仍算「对准实体」，抑制挖矿
    if (!cooled) return true;

    const tool = heldToolFromSlot(this.state.selectedSlot);
    const damage = attackDamageHalfHearts(tool);
    this.mannequinHealth = applyDamage(this.mannequinHealth, damage);
    this.attackCooldownUntil = now + LOCAL_ATTACK_COOLDOWN_MS;
    runtime.playAttackSwing();
    runtime.playMannequinHurtFeedback();
    // 沿视线推开假人（剑更远）
    const dir = runtime.getDirection();
    const impulse = knockbackImpulse(tool, [dir.x, dir.y, dir.z]);
    runtime.applyMannequinKnockback(impulse);
    this.sfx.play("dig", { rate: tool === "sword" ? 1.15 : 1.05 });

    if (isDead(this.mannequinHealth)) {
      this.finishMannequinKill(runtime, now);
    } else {
      const heartsLeft = (this.mannequinHealth / 2).toFixed(
        this.mannequinHealth % 2 === 0 ? 0 : 1,
      );
      this.targetName = `Steve · ${heartsLeft}♥`;
    }
    return true;
  }

  /**
   * 击倒：立刻消失 + 掉落当前持有物；3 秒后在附近复活。
   */
  private finishMannequinKill(
    runtime: LocalWorldRuntime,
    now: number,
  ): void {
    const eye = runtime.getMannequinEyePosition();
    const deathXZ: [number, number] = eye !== null ? [eye[0], eye[2]] : [0, 0];
    this.mannequinDeathXZ = deathXZ;
    // 掉落点：腰部附近（眼高约 1.75）
    const dropPos: [number, number, number] =
      eye !== null
        ? [eye[0], eye[1] - 1.2, eye[2]]
        : [deathXZ[0], 1, deathXZ[1]];

    const held = runtime.killMannequin();
    if (held !== null) {
      this.loot?.dropTool(held, dropPos, now, 350);
      this.sfx.play("drop");
    }
    this.mannequinRespawnAt = now + 3_000;
    this.targetName = null;
    this.showNotice(held !== null ? "击倒了 Steve · 掉落了物品" : "击倒了 Steve");
  }

  private tickMannequinRespawn(now: number): void {
    if (this.mannequinHealth > 0) return;
    if (this.mannequinRespawnAt <= 0 || now < this.mannequinRespawnAt) return;
    const runtime = this.runtime;
    const near = this.mannequinDeathXZ ?? [0, 0];
    runtime?.respawnMannequinNear(near);
    this.mannequinHealth = fullHealth();
    this.mannequinRespawnAt = 0;
    this.mannequinDeathXZ = null;
    this.showNotice("Steve 在附近重新站了起来");
  }

  private collectLoot(position: Vector3, now: number, deltaMs: number): void {
    this.loot?.updateAndCollect(position, now, deltaMs, (pickup) => {
      if (pickup.kind === "tool") {
        // 工具快捷栏已常驻；拾取仅反馈，不占资源格
        this.showNotice(
          pickup.tool === "sword" ? "捡到铁剑" : "捡到铁镐",
        );
        this.sfx.play("pickup");
        return 0;
      }
      const result = addResource(
        this.state.inventory,
        pickup.resource,
        pickup.quantity,
      );
      this.dispatch({ type: "INVENTORY_REPLACED", inventory: result.inventory });
      if (result.remainder < pickup.quantity) this.sfx.play("pickup");
      return result.remainder;
    });
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
      // 松开/失焦：只结束本段挥挖，不清除世界耐久
      this.stopActiveMining();
      return;
    }

    const requiredMs = miningDurationMs(profile, tool);
    const drop = harvestDrop(profile.drop, tool, profile);
    const priorDamage = this.durability.get(key, target.id);
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
        // 续挖：从世界耐久恢复进度（换工具只改速度）
        elapsedMs: priorDamage * requiredMs,
      });
      this.sfx.play("dig", { rate: digRateFor(drop, tool) });
      this.lastDigAt = now;
    }

    // 按当前工具速度累加 0–1 耐久
    const amount =
      requiredMs > 0 ? Math.max(0, frame.deltaMs) / requiredMs : 1;
    const damage = this.durability.apply(key, target.id, amount);
    this.dispatch({
      type: "MINING_ADVANCED",
      deltaMs: frame.deltaMs,
      targetKey: key,
      elapsedMs: damage * requiredMs,
    });
    if (now - this.lastDigAt > digIntervalMs(tool)) {
      this.sfx.play("dig", { rate: digRateFor(drop, tool) });
      this.lastDigAt = now;
    }
    if (damage >= 1) {
      this.completeMining(runtime, target, drop, now);
    }
  }

  /**
   * 裂纹：所有已损伤方块常驻显示，不依赖准星指向。
   * 方块 id 已变或已 claim 的条目跳过。
   */
  private syncBreakCrack(
    runtime: LocalWorldRuntime,
    _frame: LocalRuntimeFrame,
  ): void {
    const entries = this.durability.snapshot().flatMap((snap) => {
      if (this.claimed.has(snap.key) || !(snap.damage > 0)) return [];
      // 世界方块已变（被别的逻辑替换）则不画裂纹
      if (runtime.world.getVoxelAt(...snap.voxel) !== snap.blockId) {
        return [];
      }
      return [
        {
          key: snap.key,
          progress: snap.damage,
          voxel: snap.voxel,
        },
      ];
    });
    runtime.setBreakCracks(entries);
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
      this.durability.clear(key);
      this.stopActiveMining();
      return;
    }
    this.claimed.add(key);
    this.durability.clear(key);
    try {
      runtime.adapter.applyServerVoxelUpdate(
        runtime.world,
        target.voxel,
        LOCAL_BLOCK_IDS.air,
      );
    } catch {
      this.claimed.delete(key);
      this.stopActiveMining();
      return;
    }
    // 完整裂纹列表由 syncBreakCrack 每帧刷新；此处只播破碎特效
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
    this.stopActiveMining();
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
    this.mannequinHealth = fullHealth();
    this.mannequinRespawnAt = 0;
    this.attackCooldownUntil = 0;
    this.wasPrimaryHeld = false;
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


