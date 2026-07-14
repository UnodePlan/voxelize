# Single-player SFX sources (create.town/lab unpack)

## Procedural registry (no files)

Lab embeds a `SOUND_REGISTRY` in the main game chunk (`0emt-9~qwcfjr.js`) with
Web Audio synthesis for:

| Lab id | Used for |
|---|---|
| `block-break` | block break |
| `block-place` | mining hit / dig |
| `pickup` | item pickup |
| `tick` | UI select / extract tick |
| `thonk` | drop |
| `pop` | UI accents |
| `chest-open` / `chest-close` | inventory open/close |

Implementation lives in `apps/extraction-client/src/single/audio.ts` as a
line-faithful port of those `play:function` bodies.

## Sampled footsteps

Lab/builder footstep map (`0emt-9~qwcfjr.js` + `077c54_sssnmg.js` AudioProvider):

| Set | Lab source | Files | When |
|---|---|---|---|
| **default** | `Bn={tracks:BE,pitchVariance:.16}` — 4× OGG base64 | `footstep_default_0.ogg` … `_3.ogg` | most surfaces (current single-player always) |
| grass | `blockNames: Grass Block / Grass / …` — 4× WAV base64 | `footstep_grass_0.wav` … `_3.wav` | underfoot grass only (reserved; not wired yet) |

### Timing (builder-faithful, re-verified against 077c54)

Frame order in builder: `controls.update()` → world → **then** `updateHooks.forEach(fn)` (footsteps).

AudioProvider per-frame hook:

1. Gate: `controls.state.running && body.atRestY === -1 && !isSwimming && !busy`
2. `busy = true`
3. `pitch = 0.9 + 0.2 * random` passed to Three.Audio as **`detune` (cents)** — NOT playbackRate
4. volume `0.05`, positional `setRefDistance(20)`
5. `playAudio` Promise resolves on **`source.onended`** (full sample; OGG ≈ 0.22–0.32s at rate 1)
6. Then `setTimeout(clearBusy, sprinting ? 10 : 100)`

**Do not** map that pitch to `playbackRate` — rate 1.1 shortens the buffer and makes the next step fire early (“声音提前”).

## Policy

PRD allows porting Lab art/audio into the repo. Runtime does not fetch create.town.
