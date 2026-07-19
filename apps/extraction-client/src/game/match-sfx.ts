/**
 * 多人局内音效：复用单机 LocalSfx 合成器，补击杀/受伤组合。
 */

import { LocalSfx } from "../single/audio";

import type { MatchSfxCue } from "./match-sfx-feedback";

export class MatchSfx {
  private readonly core = new LocalSfx();
  private disposed = false;

  unlock(): void {
    if (!this.disposed) this.core.unlock();
  }

  playCue(cue: MatchSfxCue): void {
    if (this.disposed) return;
    switch (cue) {
      case "hit":
        // 出刀命中
        this.core.play("dig", { rate: 1.25, volume: 0.9 });
        break;
      case "kill":
        // 击杀：碎裂 + 短促完成音
        this.core.play("break", { rate: 0.82, volume: 1 });
        window.setTimeout(() => {
          if (!this.disposed) this.core.play("ui", { rate: 1.15, volume: 0.7 });
        }, 45);
        window.setTimeout(() => {
          if (!this.disposed) this.core.play("pickup", { rate: 0.95, volume: 0.55 });
        }, 110);
        break;
      case "hitTaken":
        // 自己挨打：低沉打击
        this.core.play("dig", { rate: 0.68, volume: 1 });
        break;
      case "death":
        this.core.play("break", { rate: 0.55, volume: 1 });
        window.setTimeout(() => {
          if (!this.disposed) this.core.play("drop", { rate: 0.9, volume: 0.85 });
        }, 80);
        break;
      case "drop":
        this.core.play("drop", { rate: 1, volume: 0.9 });
        break;
      case "pickup":
        this.core.play("pickup", { rate: 1.05, volume: 0.75 });
        break;
    }
  }

  playAll(cues: readonly MatchSfxCue[]): void {
    // 去重保序，避免同一帧 death+drop 叠两次 drop 过于吵
    const seen = new Set<MatchSfxCue>();
    for (const cue of cues) {
      if (seen.has(cue)) continue;
      seen.add(cue);
      this.playCue(cue);
    }
  }

  dispose(): void {
    this.disposed = true;
    this.core.dispose();
  }
}
