# Omnivixion — Console Spec v0.1

Working draft. Numbers committed for prototyping; expect adjustments after first playable carts.

## 0. Design ethos

- **Console ≠ display.** The console outputs a voxel field on the RD/FCC lattice. How that field is *rendered* is the emulator's choice — RDs, spheres, points, slices, ASCII. Carts run identically on every emulator that conforms to the console spec.
- **Constraint = creativity.** Tight budgets are features. Resist the urge to widen.
- **Hackable carts.** Cart format must be human-inspectable. No opaque binaries as the primary distribution form.

## 1. Versioning

- Spec version follows `MAJOR.MINOR`. v0.x is pre-stable; carts may break between minors.
- Every cart declares its target spec version in its header. Emulators must reject carts targeting an unknown major.

## 2. Lattice & display buffer

- **Lattice:** Rhombic dodecahedral (FCC). See `src/rd.ts` for canonical math.
- **Extent:** 128 lattice units per axis. Coord range `[0, 128)` per axis, unsigned.
- **Validity:** Cell `(x, y, z)` exists iff `(x + y + z) mod 2 == 0`. Half of the integer lattice is occupied; the rest is "off-grid" and writes to it are silently no-op.
- **Cell count:** 128³ / 2 = **1,048,576 cells**.
- **World origin:** Corner. Cell `(0,0,0)` is one corner of the display volume.

### Why 128³

Sweet spot for "cheap hardware" emulators. 525 KB framebuffer, ~52 K typical surface cells, ~1.25 M render triangles — phone/Pi-class GPUs hit 60 Hz easily with surface meshing. Doubling to 256³ would 8× memory and require greedy meshing as a baseline; halving to 64³ cramps game design beyond useful.

## 3. Color & palette

- **Per-cell:** 4 bits. 16 values total.
- **Value 0:** Empty (no cell drawn). Combines occupancy and color into one field.
- **Values 1–15:** Filled, palette index 1 through 15.
- **Default palette:** TBD (lock during v0.2). Tracking PICO-8's 16-color system for inspiration but probably custom. Spec slot is reserved.
- **Palette swap:** Carts may set the active palette via `pal_set(slot, rgb)`. Emulators must apply changes immediately on the next frame.
- **No HDR, no per-cell alpha.** A cell is either off, or one of 15 indexed colors. Translucency, glow, gradients are emulator embellishments.

### Display buffer size

- 1,048,576 cells × 4 bits = **524,288 bytes (512 KB)** total.
- Linear layout: address = `(x + y*128 + z*128*128) / 2` byte index, parity bit selects high/low nibble. (Final layout to be locked when the cart→emulator wire format is specified; the address scheme above is a hint, not normative.)

## 4. Coordinate system

- Coords are unsigned 7-bit (`[0, 127]`). Stored as `uint8` for alignment.
- Y is up.
- Off-grid writes (parity violation, out-of-range) are silently dropped. Carts can pre-check via `vox_is_valid(x, y, z)`.
- The 12 face-neighbor offsets (`NEIGHBOR_OFFSETS` in `rd.ts`) are part of the spec. Carts can reference them by index 0–11; semantics frozen.

## 5. Cart format & memory

- **Cart format (v0.1 text):** Carts ship as `.omni` text files — one human-inspectable file per cart, sectioned by `__name__` markers, with a TOML header. Full grammar in [`CART_FORMAT.md`](CART_FORMAT.md). The 512 KB compressed-binary form is a v0.2 concern; in v0.1 the text form is canonical and the loader parses it directly.
- **Header:** Defined as a TOML body inside the `__header__` section — see `CART_FORMAT.md` §2. Includes spec version, name, author, runtime selection, default pitch, persist-buffer flag, default palette slot, and audio bank count (reserved). The fixed 256-byte binary header described in earlier drafts is deferred to the bundled-binary form (v0.2).
- **Code:** Initially Lua via embedded `mlua`. WASM cart support deferred to v0.2 — same syscall ABI, different runtime.
- **Working RAM:** **256 KB** beyond the display buffer. Cart's variables, structures, level data live here.
- **Asset budget within cart:** Whatever fits in the 512 KB once code is compressed. A 16³ voxel "sprite" costs ~1 KB at 4 bpp; expect 100+ sprites possible.

### Why these numbers

- 512 KB cart ≈ N64-floor / Game Boy Advance territory. Big enough for chunky 3D-feeling games; small enough that a hobbyist can hand-craft assets and read the whole cart in a few minutes.
- 256 KB working RAM lets a game keep ~3× the display buffer worth of state in memory (entity lists, behavior trees, undo stacks, etc.). Smaller forces awkward streaming; larger erodes "fantasy console" feel.

## 6. Update model & syscall API

- **Default: full-refresh display.** The emulator clears the display buffer at the start of every `update()` tick. Carts redraw the full visible scene each tick — no manual cell cleanup, no surprise leftover voxels. Hobbyist-friendly default; matches the mental model of "render this frame."
- **Opt-in: persistent buffer.** A cart can call `set_persist_buffer(true)` (typically in `init()`) to keep the buffer across ticks. Best for largely-static scenes where re-stamping every cell each frame is wasteful (e.g. open-world landscapes with hundreds of thousands of cells).
- **No automatic redraw beyond the per-tick clear.** The cart is the renderer. The emulator just shows what's in the buffer.
- **Frame rate:** 30 Hz default, 60 Hz opt-in via cart header flag. Emulator runs the cart's `update()` once per tick.

### Core syscalls (v0.1)

Frozen names and shapes; behavior locked on v1.0.

```
vox_set(x, y, z, color)         -- set one cell. Off-grid → no-op.
vox_get(x, y, z) -> color       -- read one cell. Off-grid → 0.
vox_clear()                     -- zero the entire display buffer.
vox_fill(x0,y0,z0, x1,y1,z1, color) -- box fill in lattice-aligned AABB.
vox_is_valid(x, y, z) -> bool   -- parity + range check.

neighbor(x, y, z, idx) -> (x', y', z')   -- idx 0..11, returns face-neighbor.

pal_set(slot, r, g, b)          -- override palette entry. slot in 1..15.
pal_reset()                     -- restore default palette.

set_persist_buffer(persist)     -- false (default): emulator clears the buffer
                                --   each tick before update().
                                -- true: persistent buffer; cart manages cleanup.
persist_buffer() -> bool        -- read current mode.

cam_pitch(deg)                  -- set camera pitch in [0, 90]. Out-of-range clamped.
cam_pitch_get() -> deg          -- read current pitch.

btn(idx) -> bool                -- 0..9; see §7.
btnp(idx) -> bool               -- "pressed this frame" edge.

time() -> ticks                 -- monotonic frame counter.
rand() -> [0,1)                 -- deterministic PRNG, seedable.

sfx(n, ch?, offset?)            -- play SFX n. ch in 0..3, -1 = first free. n = -1 stops ch.
music(n, fade_ms?, ch_mask?)    -- start song n. ch_mask in 0..15 reserves channels.
                                -- n = -1 stops music.
audio_master(vol)               -- global volume in 0..1. Default 1.

print(...)                      -- to emulator console only, NOT the voxel field.
```

Anything not listed is not a syscall. Carts must not reach outside this surface.

## 7. Input

10-button standard input. Mapped per emulator to whatever physical controls exist.

| idx | logical name | typical mapping |
|---|---|---|
| 0 | left  | A / ← / left stick -X |
| 1 | right | D / → / left stick +X |
| 2 | down  | S / ↓ / left stick -Z |
| 3 | up    | W / ↑ / left stick +Z |
| 4 | descend | Shift / L1 / left stick -Y |
| 5 | ascend  | Space / R1 / left stick +Y |
| 6 | A | Z / South face button |
| 7 | B | X / East face button |
| 8 | C | C / West face button |
| 9 | D | V / North face button |

The 6-axis directional model maps cleanly to the lattice's 12 directions (each axis pair selects one of two zigzag sub-offsets). No analog input in v0.1.

## 8. Camera

One parameter: **pitch**. A single angle in `[0, 90]` degrees.

- `0`  → front-on view, camera looking along `-Z`. Sidescrolling / platformer feel.
- `90` → top-down, camera looking along `-Y`. Strategy / board feel.
- `30` → tilted iso-ish, the typical "voxel game" framing.

The camera traces an arc on the fixed yaw=0 plane between front and bird views. **Yaw is fixed.** v0.1 does not allow rotation around the vertical axis. (Possible v0.2 if real games demand it.)

**Default pitch:** `30°` (set when the cart starts). Cart may override in its header (`default_pitch`) and may change it dynamically during gameplay via the `cam_pitch` syscall — useful for transitions, zoom-out moments, or genre-bending mid-game.

### Projection

**Emulator's choice.** Both orthographic and perspective are conformant. Recommended: **perspective**, because the rhombic dodecahedral cell shape reads better with depth foreshortening — flat-shaded RDs viewed orthographically often look ambiguous about which face is which. A small FOV (e.g. 35°) keeps the iso-like aesthetic while preserving 3D legibility.

Emulators must position the camera so the populated region of the buffer is fully visible at any pitch. Zoom level is the emulator's call; carts cannot specify it.

### What carts must not assume

- Specific FOV, zoom level, or viewport pixel dimensions.
- Whether projection is ortho or perspective.
- Aspect ratio.
- That the entire 128³ volume is on-screen at once (only the populated region is guaranteed framed).

## 9. Audio

PICO-8-shaped 4-voice chiptune synth. Carts ship two small data banks
(sfx + music) in their `.omni` text and trigger them through `sfx()` /
`music()`. There is no lower-level note API in v0.1.

### 9.1 Voices and output

- **4 polyphonic voices**, software-mixed.
- **Output floor: 22050 Hz, mono.** Emulators may upsample, stereoize,
  anti-alias, or otherwise polish; output need only be *recognizably
  equivalent* to the waveforms specified below. This is the audio
  side of the console-≠-display contract.

### 9.2 Waveforms (4-bit per step)

Each step in an SFX names a waveform via a 4-bit field.

Values 0–7 are the fixed built-in waveforms:

| value | waveform |
|---|---|
| 0 | triangle |
| 1 | tilted saw (asymmetric) |
| 2 | saw |
| 3 | square (50% pulse) |
| 4 | pulse (~25% duty) |
| 5 | organ (stacked triangles, additive) |
| 6 | noise |
| 7 | phaser (sweeping) |

Values 8–15 are **custom instruments**: a step with waveform `8 + i`
plays SFX `i` (i.e. SFX 0..7) as a transposable voice. The host step's
pitch becomes the source SFX's starting pitch (rest of the source
plays relative to that); the host step's volume scales the source's
volume.

Recursion is illegal: SFX 0..7 may not themselves use waveforms
8–15. Steps that violate this fall back to base waveform 0 (triangle)
at load time, with a warning.

### 9.3 Effects (3-bit per step)

| value | effect |
|---|---|
| 0 | none |
| 1 | slide (portamento from previous step's pitch) |
| 2 | vibrato |
| 3 | drop (pitch falls across the step) |
| 4 | fade-in (volume ramps up across the step) |
| 5 | fade-out (volume ramps down across the step) |
| 6 | arpeggio fast (cycle prior 4 steps' pitches at high rate) |
| 7 | arpeggio slow (same, slow rate) |

### 9.4 SFX bank

- **64 slots**, indexed 0..63.
- Each SFX = **32 steps** + 4 bytes of metadata.
- Each step = **16 bits**, packed big-endian:
  - bits 15..10: pitch (6, chromatic; see below)
  - bits 9..6:   waveform (4, see §9.2)
  - bits 5..3:   volume (3, linear gain v/7; 0 = silent, 7 = max)
  - bits 2..0:   effect (3, see §9.3)
- **Pitch → frequency:** pitch `n` corresponds to MIDI note `n + 36`
  in 12-TET, A4 = 440 Hz. Pitch 0 ≈ C2 (65.4 Hz), pitch 48 ≈ C6
  (1046.5 Hz), pitch 63 ≈ D♯7 (2489.0 Hz). Total range: 5 octaves +
  3 semitones.
- **Speed:** step duration is `speed` engine ticks at the cart's
  declared `hz` (30 or 60). At hz=60, `speed=1` is 16.67 ms/step (a
  full 32-step SFX is ~533 ms); `speed=8` is 133 ms/step (~4.3 s).
- Per-SFX metadata (4 bytes):
  - byte 0: speed (1..255 ticks per step; 0 reserved)
  - byte 1: loop_start (step index 0..31)
  - byte 2: loop_end (step index 0..31; `loop_end <= loop_start` ⇒ no loop)
  - byte 3: reserved (must be 0 in v0.1)
- **Total: 64 × (32 × 2 + 4) = 4352 bytes**.

### 9.5 Music bank

- **128 patterns**, indexed `0x00..0x7F`. A *pattern* is one bar's
  worth of audio: up to 4 SFXes fired simultaneously, one per channel.
- Each pattern = **5 bytes**:
  - byte 0: flags (bit 0 = begin-loop, bit 1 = end-loop, bit 2 = stop-at-end, bits 3..7 reserved)
  - bytes 1..4: sfx ids for channels 0..3 (`0xFF` = silent channel)
- A *song* is a run of consecutive patterns starting at some index
  `n` and playing forward through patterns `n, n+1, ...` until
  playback hits a `stop-at-end` (halts) or an `end-loop` (jumps back
  to the most recent `begin-loop`, or to pattern 0 if there is none).
  A pattern advances to the next once the longest playing SFX in it
  finishes.
- Carts pack multiple songs by laying out non-overlapping runs of
  patterns with terminator flags between them; `music(n)` selects
  which song to play by its starting pattern index.
- **Total: 128 × 5 = 640 bytes**.

### 9.6 Syscalls

```
sfx(n, ch?, offset?)
  Play SFX n on channel ch. ch=-1 (default) picks the first free
  voice; if all 4 are busy, the call is silently dropped. offset is a
  starting step index 0..31 (default 0). n=-1 stops the targeted
  channel (or all channels if ch=-1).

music(n, fade_ms?, ch_mask?)
  Start the song beginning at pattern n (see §9.5). fade_ms (default
  0) cross-fades from current music in milliseconds. ch_mask
  (default 15 = all four) is a bitmask of channels the song may
  use; bits not set in ch_mask stay available to sfx(). n=-1 stops
  the current song (still respecting fade_ms).

audio_master(vol)
  Global linear gain in 0..1, clamped. Default 1.
```

### 9.7 Memory

The sfx bank (4352 B) and music bank (640 B) live in dedicated audio
RAM, not the 256 KB working RAM. **Total audio bank: 4992 bytes**,
fixed. The loader populates both banks from the cart's `__sfx__` /
`__music__` sections at startup (see `CART_FORMAT.md` §6 and §7).
The banks are read-only at runtime in v0.1; live editing of SFX data
from cart code is a v0.2 question.

### 9.8 What's not in v0.1

- Streaming PCM or sample voices (would conflict with the
  console-≠-display lens).
- Per-voice user-defined filters.
- Custom waveform shapes beyond the eight base waveforms + the
  custom-instrument trick.
- Save data / persistent state for audio.
- Stereo panning controls from the cart.
- Multiple sfx/music banks (`[audio] banks > 0` in the header is
  reserved for v0.2).

## 10. Reference emulator profiles

A conformant emulator must run any v0.x cart correctly. Performance profiles describe expected fidelity, not correctness:

| Profile | Hardware floor | Renderer | Notes |
|---|---|---|---|
| `embedded` | RPi Pico 2 W, ESP32-P4 | Software rasterize, low-res slices | May LOD-downsample to 64³ for display only. Buffer in RAM is full 128³. |
| `cheap` | RPi 4, mid Android | Surface-mesh per chunk, basic lighting | Reference profile. 60 Hz expected. |
| `standard` | Modern phone, integrated GPU laptop | Greedy-meshed, per-cell soft lighting | Expected target for most users. |
| `fancy` | Discrete GPU | Continuous geometry (spheres, metaballs, signed-distance), AA, shadows | Optional. Carts must look acceptable without it. |

## 11. Open questions (lock during v0.2)

- Default 15-color palette.
- Cart header byte layout for the bundled/compressed binary form. (The v0.1 text-format header is locked in `CART_FORMAT.md` §2.)
- Cart compression algorithm (zstd? LZ4? domain-specific RLE for voxel runs?).
- WASM cart ABI (mirror Lua syscalls 1:1, or richer?). Section reserved as `__wasm__` in `CART_FORMAT.md`.
- Save state / save data semantics.
- Multi-cart linking (cartridges that load other cartridges?). Probably no.

## 12. What's deliberately not in v0.1

- Floating point in the syscall ABI. Coords and colors are integers. PRNG is the only float-returning call.
- Per-cell metadata beyond the 4-bit value. No layers, no flags, no Z-buffer for the cart. Effects live in the cart's working RAM, not the display buffer.
- Networking.
- Physics, collision, pathfinding helpers. Library-level concerns; carts can ship their own.

---

**Status:** draft. Use the prototypes in `src/` to pressure-test. First playable cart will probably reveal what's wrong here within a week.
