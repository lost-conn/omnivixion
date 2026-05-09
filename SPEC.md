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

- **Cart binary:** Up to **512 KB** compressed. Holds code, assets, palette, header.
- **Header:** Fixed 256-byte block at start of cart. Includes spec version, name, author, default palette index, preferred camera hint, audio bank count, and code-segment offset/length. Format locked during v0.2.
- **Code:** Initially Lua via embedded `mlua`. WASM cart support deferred to v0.2 — same syscall ABI, different runtime.
- **Working RAM:** **256 KB** beyond the display buffer. Cart's variables, structures, level data live here.
- **Asset budget within cart:** Whatever fits in the 512 KB once code is compressed. A 16³ voxel "sprite" costs ~1 KB at 4 bpp; expect 100+ sprites possible.

### Why these numbers

- 512 KB cart ≈ N64-floor / Game Boy Advance territory. Big enough for chunky 3D-feeling games; small enough that a hobbyist can hand-craft assets and read the whole cart in a few minutes.
- 256 KB working RAM lets a game keep ~3× the display buffer worth of state in memory (entity lists, behavior trees, undo stacks, etc.). Smaller forces awkward streaming; larger erodes "fantasy console" feel.

## 6. Update model & syscall API

- **Display buffer is persistent.** Frame N's cells carry into frame N+1 unless overwritten. Carts opt into clearing.
- **No automatic redraw.** The cart is the renderer. The emulator just shows what's in the buffer.
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

cam_pitch(deg)                  -- set camera pitch in [0, 90]. Out-of-range clamped.
cam_pitch_get() -> deg          -- read current pitch.

btn(idx) -> bool                -- 0..9; see §7.
btnp(idx) -> bool               -- "pressed this frame" edge.

time() -> ticks                 -- monotonic frame counter.
rand() -> [0,1)                 -- deterministic PRNG, seedable.

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

TBD. v0.2 target: 4-channel chiptune-style synth, 16 KB sound bank, drawn from cart memory. Not blocking v0.1 visual playables.

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
- Cart header byte layout.
- Cart compression algorithm (zstd? LZ4? domain-specific RLE for voxel runs?).
- Audio system shape.
- WASM cart ABI (mirror Lua syscalls 1:1, or richer?).
- Save state / save data semantics.
- Multi-cart linking (cartridges that load other cartridges?). Probably no.

## 12. What's deliberately not in v0.1

- Floating point in the syscall ABI. Coords and colors are integers. PRNG is the only float-returning call.
- Per-cell metadata beyond the 4-bit value. No layers, no flags, no Z-buffer for the cart. Effects live in the cart's working RAM, not the display buffer.
- Networking.
- Physics, collision, pathfinding helpers. Library-level concerns; carts can ship their own.

---

**Status:** draft. Use the prototypes in `src/` to pressure-test. First playable cart will probably reveal what's wrong here within a week.
