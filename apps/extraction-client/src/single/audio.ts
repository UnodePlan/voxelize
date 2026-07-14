/**
 * create.town/builder（与 lab 共用 AudioProvider）音效移植。
 *
 * 来源 chunk：`077c54_sssnmg.js` AudioProvider + `0emt-9~qwcfjr.js` pickFootstepSound
 *
 * 脚步关键（builder 原样）：
 *   if (running && atRestY===-1 && !swimming && !busy) {
 *     busy = true
 *     play(track, volume=0.05, pitch=0.9+0.2*random)
 *       .then(() => setTimeout(() => busy=false, sprinting ? 10 : 100))
 *   }
 * playAudio 的 Promise 在 source.onended 时 resolve —— 即「整段播完」再加间隔。
 * 默认采样 4×OGG 时长约 0.22–0.32s → 走路有效步频约 3 步/秒。
 */

import footstep0 from "../assets/single/sfx/footstep_default_0.ogg";
import footstep1 from "../assets/single/sfx/footstep_default_1.ogg";
import footstep2 from "../assets/single/sfx/footstep_default_2.ogg";
import footstep3 from "../assets/single/sfx/footstep_default_3.ogg";

/** 对外事件 id（映射到 Lab 注册表） */
export type LocalSfxId =
  | "dig"
  | "break"
  | "pickup"
  | "drop"
  | "ui"
  | "inventoryOpen"
  | "inventoryClose"
  | "extractTick"
  | "extractDone"
  | "step"
  | "select";

type LabSoundId =
  | "block-break"
  | "block-place"
  | "tick"
  | "thonk"
  | "pop"
  | "pickup"
  | "chest-open"
  | "chest-close";

type PlayOpts = {
  volume?: number;
  pitch?: number;
  bypassCooldown?: boolean;
  /** builder：冲刺间隔 10ms，走路 100ms（均在采样播完后） */
  sprinting?: boolean;
};

const FOOTSTEP_URLS = [footstep0, footstep1, footstep2, footstep3];
/** builder walk 冷却：播完后再等（077c54 AudioProvider） */
const FOOTSTEP_GAP_WALK_MS = 100;
const FOOTSTEP_GAP_SPRINT_MS = 10;
/**
 * builder 注册表 `walk.volume: .05`（PositionalAudio + setRefDistance(20)）。
 * 本地是 2D destination，完全 0.05 偏小，略抬到 0.08 保持可听但不抢拍。
 */
const FOOTSTEP_BASE_VOLUME = 0.08;

const LAB_COOLDOWN_MS: Record<LabSoundId, number> = {
  "block-break": 35,
  "block-place": 35,
  tick: 30,
  thonk: 30,
  pop: 30,
  pickup: 20,
  "chest-open": 20,
  "chest-close": 20,
};

/** 我方事件 → Lab 音效 */
const EVENT_TO_LAB: Record<LocalSfxId, LabSoundId | "step" | "extract-done"> = {
  dig: "block-place",
  break: "block-break",
  pickup: "pickup",
  drop: "thonk",
  ui: "tick",
  select: "tick",
  inventoryOpen: "chest-open",
  inventoryClose: "chest-close",
  extractTick: "tick",
  extractDone: "extract-done",
  step: "step",
};

export class LocalSfx {
  private context: AudioContext | null = null;
  private unlocked = false;
  private disposed = false;
  private lastPlayAt = new Map<string, number>();
  private footstepBuffers: AudioBuffer[] | null = null;
  private footstepLoading: Promise<void> | null = null;
  /** builder AudioProvider 的 busy 门闩 A */
  private footstepBusy = false;

  unlock(): void {
    if (this.disposed) return;
    const ctx = this.ensureContext();
    if (ctx.state === "suspended") void ctx.resume();
    this.unlocked = true;
    void this.ensureFootsteps(ctx);
  }

  play(
    id: LocalSfxId,
    options: { volume?: number; rate?: number; sprinting?: boolean } = {},
  ): void {
    if (this.disposed || !this.unlocked) return;
    const mapped = EVENT_TO_LAB[id];
    const pitch = options.rate ?? 1;
    const volume = options.volume ?? 1;

    if (mapped === "step") {
      void this.playFootstep({
        volume,
        pitch,
        sprinting: options.sprinting === true,
      });
      return;
    }
    if (mapped === "extract-done") {
      // Lab 无撤离完成专用音，用 tick + pickup 组合接近“完成反馈”
      this.playLab("tick", { volume, pitch: 0.9 });
      window.setTimeout(
        () => this.playLab("pop", { volume, pitch: 1.05 }),
        60,
      );
      window.setTimeout(
        () => this.playLab("pickup", { volume, pitch: 1.1 }),
        140,
      );
      return;
    }
    this.playLab(mapped, { volume, pitch });
  }

  dispose(): void {
    this.disposed = true;
    this.footstepBusy = false;
    if (this.context !== null) {
      void this.context.close();
      this.context = null;
    }
    this.footstepBuffers = null;
  }

  private playLab(id: LabSoundId, opts: PlayOpts): void {
    const nowMs = performance.now();
    const last = this.lastPlayAt.get(id) ?? 0;
    if (!opts.bypassCooldown && nowMs - last < LAB_COOLDOWN_MS[id]) return;
    this.lastPlayAt.set(id, nowMs);

    const ctx = this.ensureContext();
    if (ctx.state === "suspended") void ctx.resume();
    const params = { volume: opts.volume ?? 1, pitch: opts.pitch ?? 1 };
    LAB_PLAYERS[id](ctx, params);
  }

  /**
   * builder 脚步（逐行对齐 077c54 AudioProvider）：
   *   A=!0
   *   P=.9+.2*Math.random()
   *   U(track,.05,pos,P).then(()=>setTimeout(()=>A=!1, sprint?10:100))
   * U 在 source.onended 时 resolve —— 时长由采样本身决定，不用 playbackRate 加速。
   */
  private async playFootstep(opts: PlayOpts): Promise<void> {
    if (this.footstepBusy || this.disposed) return;
    this.footstepBusy = true;

    try {
      const ctx = this.ensureContext();
      await this.ensureFootsteps(ctx);
      if (
        this.disposed ||
        this.footstepBuffers === null ||
        this.footstepBuffers.length === 0
      ) {
        return;
      }
      if (ctx.state === "suspended") void ctx.resume();

      const buffer =
        this.footstepBuffers[
          Math.floor(Math.random() * this.footstepBuffers.length)
        ];
      const source = ctx.createBufferSource();
      source.buffer = buffer;

      // builder：C.detune = w，w = 0.9+0.2*random（单位是 cents，几乎无音高变化）
      // 关键：绝不能用 playbackRate=0.9~1.1，那会缩短采样 → 下一步提前触发（用户听成“偏早”）
      const detuneCents = 0.9 + 0.2 * Math.random();
      source.playbackRate.setValueAtTime(1, ctx.currentTime);
      source.detune.setValueAtTime(detuneCents, ctx.currentTime);

      const gain = ctx.createGain();
      gain.gain.setValueAtTime(
        FOOTSTEP_BASE_VOLUME * (opts.volume ?? 1),
        ctx.currentTime,
      );
      source.connect(gain);
      gain.connect(ctx.destination);

      // 等 onended ≡ builder playAudio Promise resolve（整段播完）
      await new Promise<void>((resolve) => {
        let settled = false;
        const done = () => {
          if (settled) return;
          settled = true;
          resolve();
        };
        source.onended = done;
        source.start();
        // 兜底：若 onended 未触发，按真实时长解锁（rate=1 时 = buffer.duration）
        window.setTimeout(done, Math.ceil(buffer.duration * 1000) + 50);
      });

      const gapMs = opts.sprinting
        ? FOOTSTEP_GAP_SPRINT_MS
        : FOOTSTEP_GAP_WALK_MS;
      await sleepMs(gapMs);
    } finally {
      this.footstepBusy = false;
    }
  }

  private ensureContext(): AudioContext {
    if (this.context !== null) return this.context;
    const AudioCtx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext: typeof AudioContext })
        .webkitAudioContext;
    this.context = new AudioCtx();
    return this.context;
  }

  private async ensureFootsteps(ctx: AudioContext): Promise<void> {
    if (this.footstepBuffers !== null) return;
    if (this.footstepLoading !== null) {
      await this.footstepLoading;
      return;
    }
    this.footstepLoading = (async () => {
      const buffers: AudioBuffer[] = [];
      for (const url of FOOTSTEP_URLS) {
        try {
          const response = await fetch(url);
          const raw = await response.arrayBuffer();
          const decoded = await ctx.decodeAudioData(raw.slice(0));
          buffers.push(decoded);
        } catch {
          // 单条失败不阻断
        }
      }
      this.footstepBuffers = buffers;
    })();
    await this.footstepLoading;
  }
}

function sleepMs(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

type LabParams = { volume: number; pitch: number };

const vol = (base: number, p: LabParams) => base * p.volume;

/** Lab SOUND_REGISTRY play 函数原样移植 */
const LAB_PLAYERS: Record<
  LabSoundId,
  (ctx: AudioContext, p: LabParams) => void
> = {
  "block-break"(ctx, p) {
    const e = ctx.currentTime;
    const t = p.pitch * (0.9 + 0.2 * Math.random());
    const w = ctx.createBuffer(1, 0.1 * ctx.sampleRate, ctx.sampleRate);
    const Q = w.getChannelData(0);
    for (let A = 0; A < Q.length; A += 1) {
      const B = Math.exp(-(6 * (A / Q.length)));
      Q[A] = (2 * Math.random() - 1) * B;
    }
    const f = ctx.createBufferSource();
    f.buffer = w;
    f.playbackRate.setValueAtTime(t, e);
    const P = ctx.createBiquadFilter();
    P.type = "lowpass";
    P.frequency.setValueAtTime(800, e);
    const D = ctx.createGain();
    D.gain.setValueAtTime(vol(0.6, p), e);
    f.connect(P);
    P.connect(D);
    D.connect(ctx.destination);
    f.start(e);
    f.stop(e + 0.1);
  },

  "block-place"(ctx, p) {
    const e = ctx.currentTime;
    const t = p.pitch * (0.7 + 0.15 * Math.random());
    const w = ctx.createBuffer(1, 0.08 * ctx.sampleRate, ctx.sampleRate);
    const Q = w.getChannelData(0);
    for (let A = 0; A < Q.length; A += 1) {
      const B = Math.exp(-(8 * (A / Q.length)));
      Q[A] = (2 * Math.random() - 1) * B;
    }
    const f = ctx.createBufferSource();
    f.buffer = w;
    f.playbackRate.setValueAtTime(t, e);
    const P = ctx.createBiquadFilter();
    P.type = "lowpass";
    P.frequency.setValueAtTime(400, e);
    const D = ctx.createGain();
    D.gain.setValueAtTime(vol(0.55, p), e);
    f.connect(P);
    P.connect(D);
    D.connect(ctx.destination);
    f.start(e);
    f.stop(e + 0.08);
  },

  tick(ctx, p) {
    const e = ctx.currentTime;
    const t = ctx.createOscillator();
    const w = ctx.createGain();
    t.type = "sine";
    t.frequency.setValueAtTime(1800 * p.pitch, e);
    t.frequency.exponentialRampToValueAtTime(800 * p.pitch, e + 0.015);
    w.gain.setValueAtTime(vol(0.4, p), e);
    w.gain.exponentialRampToValueAtTime(0.001, e + 0.015);
    t.connect(w);
    w.connect(ctx.destination);
    t.start(e);
    t.stop(e + 0.015);
  },

  thonk(ctx, p) {
    const e = ctx.currentTime;
    const t = ctx.createOscillator();
    const w = ctx.createGain();
    t.type = "sine";
    t.frequency.setValueAtTime(150 * p.pitch, e);
    t.frequency.exponentialRampToValueAtTime(60 * p.pitch, e + 0.05);
    w.gain.setValueAtTime(vol(0.5, p), e);
    w.gain.exponentialRampToValueAtTime(0.001, e + 0.05);
    t.connect(w);
    w.connect(ctx.destination);
    t.start(e);
    t.stop(e + 0.05);
  },

  pop(ctx, p) {
    const e = ctx.currentTime;
    const t = ctx.createOscillator();
    const w = ctx.createGain();
    t.type = "sine";
    t.frequency.setValueAtTime(400 * p.pitch, e);
    t.frequency.exponentialRampToValueAtTime(150 * p.pitch, e + 0.04);
    w.gain.setValueAtTime(vol(0.35, p), e);
    w.gain.exponentialRampToValueAtTime(0.001, e + 0.04);
    t.connect(w);
    w.connect(ctx.destination);
    t.start(e);
    t.stop(e + 0.04);
  },

  pickup(ctx, p) {
    const e = ctx.currentTime;
    const t = p.pitch * (0.95 + 0.1 * Math.random());
    const w = ctx.createOscillator();
    const Q = ctx.createGain();
    w.type = "sine";
    w.frequency.setValueAtTime(460 * t, e);
    w.frequency.exponentialRampToValueAtTime(180 * t, e + 0.045);
    Q.gain.setValueAtTime(vol(0.25, p), e);
    Q.gain.exponentialRampToValueAtTime(0.001, e + 0.05);
    w.connect(Q);
    Q.connect(ctx.destination);
    w.start(e);
    w.stop(e + 0.05);
  },

  "chest-open"(ctx, p) {
    const e = ctx.currentTime;
    const t = p.pitch * (0.9 + 0.2 * Math.random());
    // Lab BC helper: square bandpass sweep
    labCreak(ctx, e, 200 * t, 340 * t, 0.08, vol(0.12, p));
    labCreak(ctx, e + 0.06, 260 * t, 420 * t, 0.1, vol(0.14, p));
    const w = ctx.createBuffer(1, 0.06 * ctx.sampleRate, ctx.sampleRate);
    const Q = w.getChannelData(0);
    for (let A = 0; A < Q.length; A += 1)
      Q[A] = (2 * Math.random() - 1) * Math.exp((-A / Q.length) * 8);
    const f = ctx.createBufferSource();
    f.buffer = w;
    const P = ctx.createBiquadFilter();
    P.type = "lowpass";
    P.frequency.setValueAtTime(500, e);
    const D = ctx.createGain();
    D.gain.setValueAtTime(vol(0.08, p), e);
    D.gain.exponentialRampToValueAtTime(0.001, e + 0.06);
    f.connect(P);
    P.connect(D);
    D.connect(ctx.destination);
    f.start(e);
    f.stop(e + 0.06);
  },

  "chest-close"(ctx, p) {
    const e = ctx.currentTime;
    const t = p.pitch * (0.9 + 0.2 * Math.random());
    const w = ctx.createOscillator();
    const Q = ctx.createGain();
    const f = ctx.createBiquadFilter();
    w.type = "square";
    w.frequency.setValueAtTime(160 * t, e);
    w.frequency.exponentialRampToValueAtTime(80 * t, e + 0.07);
    f.type = "lowpass";
    f.frequency.setValueAtTime(800, e);
    f.frequency.exponentialRampToValueAtTime(200, e + 0.07);
    f.Q.setValueAtTime(2, e);
    Q.gain.setValueAtTime(vol(0.2, p), e);
    Q.gain.exponentialRampToValueAtTime(0.001, e + 0.1);
    w.connect(f);
    f.connect(Q);
    Q.connect(ctx.destination);
    w.start(e);
    w.stop(e + 0.1);
    const P = ctx.createBuffer(1, 0.08 * ctx.sampleRate, ctx.sampleRate);
    const D = P.getChannelData(0);
    for (let A = 0; A < D.length; A += 1)
      D[A] = (2 * Math.random() - 1) * Math.exp((-A / D.length) * 12);
    const C = ctx.createBufferSource();
    C.buffer = P;
    const v = ctx.createBiquadFilter();
    v.type = "lowpass";
    v.frequency.setValueAtTime(600, e);
    v.frequency.exponentialRampToValueAtTime(150, e + 0.08);
    const o = ctx.createGain();
    o.gain.setValueAtTime(vol(0.15, p), e);
    o.gain.exponentialRampToValueAtTime(0.001, e + 0.08);
    C.connect(v);
    v.connect(o);
    o.connect(ctx.destination);
    C.start(e);
    C.stop(e + 0.08);
  },
};

/** Lab `BC` helper used by chest-open */
function labCreak(
  ctx: AudioContext,
  t0: number,
  f0: number,
  f1: number,
  duration: number,
  volume: number,
): void {
  const Q = ctx.createOscillator();
  const f = ctx.createGain();
  const P = ctx.createBiquadFilter();
  Q.type = "square";
  Q.frequency.setValueAtTime(f0, t0);
  Q.frequency.exponentialRampToValueAtTime(f1, t0 + duration);
  P.type = "bandpass";
  P.frequency.setValueAtTime(1.5 * f0, t0);
  P.frequency.exponentialRampToValueAtTime(1.2 * f1, t0 + duration);
  P.Q.setValueAtTime(5, t0);
  f.gain.setValueAtTime(0, t0);
  f.gain.linearRampToValueAtTime(volume, t0 + 0.003);
  f.gain.setValueAtTime(volume, t0 + 0.3 * duration);
  f.gain.exponentialRampToValueAtTime(0.001, t0 + duration);
  Q.connect(P);
  P.connect(f);
  f.connect(ctx.destination);
  Q.start(t0);
  Q.stop(t0 + duration);
}
