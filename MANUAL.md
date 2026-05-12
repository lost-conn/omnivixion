# omnivixion cart authoring manual

A friendly guide to writing `.omni` carts. Pairs with two reference docs:

- [`SPEC.md`](SPEC.md) — the formal console spec. Numbers, syscalls, conformance.
- [`CART_FORMAT.md`](CART_FORMAT.md) — the `.omni` text-format grammar. Section markers, validation rules.

This file is the part that says **how to actually use the thing**. It assumes you've at least skimmed the README so you know what omnivixion is (a voxel fantasy console, 128³ rhombic-dodecahedral display, PICO-8-style ethos in 3D).

---

## 1. Hello, voxel

The smallest interesting cart. Save as `carts/hello.omni`:

```
omnivixion cart v0.1

__header__
[meta]
spec   = "0.1"
name   = "hello"
author = "you"

[runtime]
lang = "lua"
hz   = 30

[display]
default_pitch  = 30
persist_buffer = false

__lua__
function update(dt)
  local t = time()
  for dx = 0, 3 do
    for dy = 0, 3 do
      for dz = 0, 3 do
        if (dx + dy + dz) % 2 == 0 then
          vox_set(64 + dx, 64 + dy, 64 + dz, 1 + ((t + dx + dy + dz) % 15))
        end
      end
    end
  end
end
```

Run:

```sh
cargo run --release -- --cart carts/hello.omni
```

You should see a 4³ block of cells near the world center, cycling through the palette every frame.

The structure is always the same:

1. **Magic line** identifying the format version.
2. **`__header__`** — TOML metadata.
3. **`__lua__`** — your code. The chunk runs once at load. If you define a `function init() ... end` it's called once before the first frame; `function update(dt) ... end` runs every tick.
4. (optional) `__sprites__`, `__palette__`, `__sfx__`, `__music__`, `__data NAME__` — assets.

That's it. No project files, no build step, no manifest beyond the header. One file = one cart.

---

## 2. The lattice in your head

omnivixion's display is 128 × 128 × 128 = 2,097,152 *integer* lattice cells, but only **half** of them are valid: a cell `(x, y, z)` exists iff `(x + y + z) mod 2 == 0`. The other half are off-grid; writes to them are silently dropped.

That parity rule is the load-bearing constraint of the platform. A few consequences:

- **Every neighbor lives at distance √2.** From any valid cell, the 12 face-neighbors are the offsets `(±1, ±1, 0)`, `(±1, 0, ±1)`, `(0, ±1, ±1)`. There's no axis-aligned step at lattice resolution. Pure +X movement at native resolution doesn't exist.
- **Two ways to deal with that.** Either work at a coarser **tile abstraction** — pick a grid spacing of 2 or 4 lattice cells per "tile" so you can move tile-by-tile axially — or **zigzag** — move two face-neighbor steps that net to an axis (e.g. +X+Y then +X−Y is a net +2X). `carts/pacman.omni` does the first for the maze grid and the second for the pellet-eating animation that slides between tiles.
- **Writes drop silently.** `vox_set(0, 0, 1, 7)` is a no-op because `(0+0+1) mod 2 == 1`. If you want to be sure: `if vox_is_valid(x, y, z) then vox_set(...) end`.
- **Y is up.** X and Z are the "ground" axes. Coordinates are unsigned 7-bit, range `[0, 127]`.

The 12 face-neighbor offsets are part of the spec — `neighbor(x, y, z, idx)` with `idx` in 0..11 returns the neighbor's coordinates. The order is frozen.

---

## 3. Drawing voxels

The display is a 4-bits-per-cell field. Color 0 means "empty"; colors 1..15 are filled at that palette slot.

| call | what it does |
|---|---|
| `vox_set(x, y, z, color)` | Set one cell. Off-grid → no-op. |
| `vox_get(x, y, z) -> color` | Read one cell. Off-grid → 0. |
| `vox_clear()` | Zero the entire display buffer. |
| `vox_fill(x0, y0, z0, x1, y1, z1, color)` | Box-fill an AABB. Off-grid cells inside the box are skipped. |
| `vox_is_valid(x, y, z) -> bool` | Parity + range check. |
| `neighbor(x, y, z, idx) -> nx, ny, nz` | Face-neighbor lookup, `idx` in 0..11. |

**Update model:** by default, the emulator clears the display buffer at the start of every `update()` so your cart can redraw the scene from scratch each tick — same mental model as a render loop. If your scene is mostly static (terrain, backdrops), call `set_persist_buffer(true)` once in `init()` and the emulator will keep the buffer between ticks; you become responsible for clearing what you want gone.

`carts/demo.omni` opts into persist mode because the terrain is ~750K cells set once and only the beacon flashes per-frame. `carts/pacman.omni` uses the default (full-refresh) because the maze, ghosts, and pellets are tiny and re-rendering each frame is fine.

Common idiom — draw a sphere of radius `r` at `(cx, cy, cz)`:

```lua
for dx = -r, r do
  for dy = -r, r do
    for dz = -r, r do
      if dx*dx + dy*dy + dz*dz <= r*r then
        vox_set(cx + dx, cy + dy, cz + dz, color)
      end
    end
  end
end
```

The parity rule means about half the cells in that box are skipped — that's expected. The result still reads as a sphere.

---

## 4. Sprites

Sprites are cubic voxel patterns you stamp into the display buffer. 256 slots, sizes 2 / 4 / 8 / 16. Color 0 in a sprite is transparent.

You author sprites in a `__sprites__` section — text glyph grids, one per Y-slice:

```
__sprites__
sprite 0 wall 4 4 4
y=0
1 . 1 .
. 1 . 1
1 . 1 .
. 1 . 1
y=1
. 1 . 1
1 . 1 .
. 1 . 1
1 . 1 .
y=2
1 . 1 .
. 1 . 1
1 . 1 .
. 1 . 1
y=3
. 1 . 1
1 . 1 .
. 1 . 1
1 . 1 .
```

Glyphs:
- `.` = off-parity (must appear at every position where `(x+y+z)` is odd)
- `0` = on-parity empty (carve a hole)
- `1`..`9`, `a`..`f` = palette index 1..15 (lowercase only)

The loader registers each declared sprite via `spr_load(id, size, packed_bytes)` before your `init()` runs. Then in your code:

```lua
spr_draw(0, 32, 4, 32)   -- stamp sprite 0 at world cell (32, 4, 32)
spr_clear(0)             -- drop sprite 0 from the bank
```

You can also build sprites at runtime in Lua. `carts/pacman.omni` does this with a `solid_sprite(size, color)` helper because it only needs solid-color blocks.

Full grammar in [`CART_FORMAT.md`](CART_FORMAT.md) §5.

---

## 5. Text

Built-in 5×7 voxel font, full printable ASCII. Three orientations:

| orientation | advance axis | glyph height | best for |
|---|---|---|---|
| `"xy_wall"` (default) | +X | +Y | a billboard / wall facing the camera |
| `"xz_floor"` | +X | −Z | text laid flat on the floor, readable from above |
| `"zy_wall"` | +Z | +Y | a side wall facing +X |

```lua
text_draw("HELLO", 16, 60, 32, 11)
text_draw_axis("PRESS Z", 16, 4, 30, 10, "xz_floor")
```

Glyphs auto-snap by ±1 along the depth axis to satisfy parity, so text is 1–2 cells thick (a wobbly engraving look — leans into the lattice rather than fighting it).

`text_advance()` returns the per-glyph step along the right axis (5-cell glyph + 1-cell gap = 6 cells); `text_height()` returns 7. Useful when you're computing layout.

Pacman uses `xz_floor` for the title and HUD because they read best from the typical 30°-pitch camera.

---

## 6. Input

10 buttons, `idx` 0..9. The 6-axis directional model maps cleanly to the lattice.

| idx | name | typical key |
|---|---|---|
| 0 | left | A |
| 1 | right | D |
| 2 | down | S |
| 3 | up | W |
| 4 | descend | Shift |
| 5 | ascend | Space |
| 6 | A | Z |
| 7 | B | X |
| 8 | C | C |
| 9 | D | V |

Two flavors:

- `btn(idx) -> bool` — held this frame.
- `btnp(idx) -> bool` — pressed this frame (edge-triggered, fires exactly once per press).

Use `btnp` for one-shot actions (jump, fire, menu confirm). Use `btn` for continuous actions (movement, hold-to-charge).

Pacman's input is a *sticky desired-direction* idiom that's worth stealing:

```lua
if     btn(3) then desired_dir = { 0, -1 }   -- up
elseif btn(2) then desired_dir = { 0,  1 }   -- down
elseif btn(0) then desired_dir = {-1,  0 }   -- left
elseif btn(1) then desired_dir = { 1,  0 }   -- right
end

-- Try to turn into desired_dir at the next tile boundary; otherwise keep
-- moving in current_dir. If neither works, stop.
```

The cart only checks input once per logical tick, then commits a movement when it's at a tile boundary. Game loops with sub-tile animation should look at this pattern.

---

## 7. Audio

Four-voice chiptune synth. The model is PICO-8-shaped: you author short SFX (32 steps each) plus optional songs that chain SFX together as patterns. Carts trigger them with `sfx()` and `music()`.

### SFX

Each SFX is 32 16-bit steps + 4 bytes of metadata, declared in a `__sfx__` section. Each step packs:

- **6 bits pitch** (chromatic; pitch `n` → MIDI `n+36`; pitch 0 ≈ C2, pitch 48 ≈ C6)
- **4 bits waveform** (0=triangle, 1=tilted-saw, 2=saw, 3=square, 4=pulse, 5=organ, 6=noise, 7=phaser; 8..15 = custom instruments — see below)
- **3 bits volume** (0 = silent, 7 = max)
- **3 bits effect** (0=none, 1=slide, 2=vibrato, 3=drop, 4=fade-in, 5=fade-out, 6=arp-fast, 7=arp-slow)

Encoded as a 4-character hex token. A C3 square wave at full volume with no effect is `(12<<10)|(3<<6)|(7<<3)|0` = `0x30f8`.

Example — a short ascending square-wave chirp:

```
__sfx__
sfx 00 chirp
  speed=4 loop=none
  30f8 34f8 38f8 3cf8 40f8 44f8 48f8 4cf8
  0000 0000 0000 0000 0000 0000 0000 0000
  0000 0000 0000 0000 0000 0000 0000 0000
  0000 0000 0000 0000 0000 0000 0000 0000
```

`speed` is engine ticks per step (1..255). At the engine's 60 Hz tick, `speed=4` is ~67 ms/step → an 8-step chirp lasts ~530 ms. `loop=A..B` makes the SFX loop steps A..B forever; `loop=none` plays once. SFX 00..3F (64 slots).

Trigger from Lua:

```lua
sfx(0)                   -- play SFX 0 on first free voice
sfx(0, 0)                -- play SFX 0 specifically on channel 0 (stomps anything there)
sfx(0, -1, 8)            -- play SFX 0, starting at step 8 instead of 0
sfx(-1)                  -- stop ALL non-music voices
sfx(-1, 2)               -- stop just channel 2
```

Pin to a specific channel when you want each new trigger to interrupt the previous one (e.g. a pacman chomp should always play on its dedicated voice). Use the default `ch=-1` when overlap is fine.

### Music

A `__music__` section is up to 128 *patterns*, each one bar that fires up to 4 SFX simultaneously (one per channel) plus a 1-byte flags field:

```
__music__
# id flags ch0 ch1 ch2 ch3
00:    01    02  03  ff  ff   # begin-loop here (flag bit 0)
01:    00    02  04  ff  ff
02:    00    02  03  ff  ff
03:    02    05  06  ff  ff   # end-loop → jumps back to pattern 00 (flag bit 1)
```

`ff` on a channel means "silent for this pattern." Flag bits: `0x01` begin-loop, `0x02` end-loop, `0x04` stop-at-end.

A *song* is a run of consecutive patterns. `music(0)` starts at pattern 0 and runs forward through patterns until it hits a flag terminator. To pack multiple songs in one bank, lay them out as non-overlapping runs:

```lua
music(0x00)              -- title song (patterns 00..03)
music(0x04)              -- battle song (patterns 04..0b)
music(0x0c)              -- game-over jingle (patterns 0c..0d, stop-at-end)
music(0x00, 500)         -- start title with a 500 ms cross-fade from current music
music(0x04, 0, 0x03)     -- start battle, but ch_mask=0x03 reserves only channels 0+1
                         -- (channels 2+3 stay free for sfx())
music(-1)                -- stop music
music(-1, 1000)          -- stop with a 1 s fade-out
```

By default music claims all 4 channels; `ch_mask` lets you reserve only some. Channels not in the mask stay available to `sfx()` calls — that's how you fire one-shot sound effects on top of background music.

`carts/jukebox.omni` is a worked example of all of this.

### Custom instruments

A step's waveform field is 4 bits = 16 values, but only 0..7 are built-in waveforms. Values **8..15** mean "play SFX `(n-8)` as the voice for this step." So waveform 8 = use SFX 0 as the instrument, 9 = SFX 1, etc.

The trick: you author a short tonal envelope (e.g. a 4-step decay with a hard attack) in SFX 0, then use waveform=8 in some other SFX's steps to play that envelope at the host step's pitch. The envelope transposes with you.

This gives you 8 user-definable "instruments" without spending any new memory — they're just the first 8 SFX slots, also addressable as voices. `carts/inst.omni` has a worked example. (Constraint: SFX 0..7 themselves can't use waveforms 8..15. The loader warns and clamps them to triangle if you try.)

### Master volume

`audio_master(vol)` — `vol` is 0..1, default 1. Use it for a pause-screen mute, an in-cart volume slider, or to pre-attenuate if your cart's mix is hot. The emulator's default master is set conservatively; carts that want louder can raise it, but consider the user's headphones first.

### When to use what

- Single sound effects (chomp, jump, hit) → `sfx()`.
- Background music → author patterns, call `music()` once at scene start.
- Ambient texture (engine drone, wind) → SFX with `loop=A..B` and a long enough loop to feel continuous, triggered with `sfx()` once.
- Sound that depends on game state (charging, pitch-bend on speed) → harder right now; v0.1 has no per-voice pitch syscall. Workaround: author multiple SFX at different pitches and switch.

Audio is hand-edited as hex today. A tracker UI is on the v0.2 wishlist.

---

## 8. Camera

The emulator owns the camera. Carts can hint **pitch only** — one angle in `[0, 90]` degrees. Yaw is fixed in v0.1.

- `0°` → front-on, looking along `−Z`. Sidescrolling / platformer.
- `30°` → tilted iso-ish. Good default for most "voxel game" framing.
- `90°` → top-down, looking along `−Y`. Strategy / board games.

```lua
cam_pitch(30.0)             -- set
local p = cam_pitch_get()   -- read
cam_pitch(p + 10.0)         -- nudge up
```

The cart can change pitch dynamically — useful for cinematic transitions (zoom out on death, tilt down for a menu) or genre-bending mid-game.

What carts **must not** assume: specific FOV, zoom level, projection (ortho or perspective is the emulator's choice), aspect ratio, or that the entire 128³ volume is on-screen at once. The emulator guarantees the populated region is framed; that's it.

The default header field `display.default_pitch` sets the initial pitch when the cart starts. `carts/pacman.omni` uses `default_pitch = 75` for a near-top-down view; `carts/demo.omni` uses 30°.

---

## 9. Palette

16 slots, 0..15. **Slot 0 is reserved as "empty" — you can't draw with it.** Slots 1..15 are the colors you actually have.

The default palette (in `src/console.rs::DEFAULT_PALETTE`) is a working set, not yet locked for v1.0. Carts that care should override what they need:

```lua
pal_set(1, 0.95, 0.20, 0.30)   -- slot 1 = red (0..1 RGB)
pal_set(7, 0.10, 0.80, 0.90)   -- slot 7 = teal
pal_reset()                    -- back to defaults
```

Or override at load time via the `__palette__` section (hex RGB, indices 1..15):

```
__palette__
1: 1a1c2c
2: 5d275d
15: f4f4f4
```

Index 0 in the section is tolerated but ignored — it's the empty sentinel.

---

## 10. Cart structure recap

The full cart lifecycle:

1. The emulator parses the `.omni` file. Header validation, syscall table prepared. If anything's malformed, the cart is rejected with a line-numbered error.
2. The Lua chunk runs once at load. Top-level statements execute; functions get registered.
3. Sprite, palette, sfx, and music banks are loaded from their `__...__` sections.
4. `set_persist_buffer(self.header.display.persist_buffer)` and `cam_pitch(self.header.display.default_pitch)` are applied.
5. `init()` is called once if you defined one.
6. `update(dt)` is called every logic tick. Default 60 Hz; cart can declare `hz = 30` in the header for a slower clock. (Note: the engine loop in v0.1 is hardcoded to 60 Hz regardless of cart hz; this will be honored in a later revision.)
7. The display buffer is auto-cleared between ticks unless you opted into `set_persist_buffer(true)`.

Header schema lives in [`CART_FORMAT.md`](CART_FORMAT.md) §2. Required fields: `meta.spec`, `meta.name`, `meta.author`. Everything else has sensible defaults.

The Lua sandbox exposes the syscalls plus a conservative stdlib subset: `string`, `table`, `math`, `pairs`, `ipairs`, `next`, `select`, `tostring`, `tonumber`, `type`, `error`, `assert`, `pcall`, `xpcall`, `unpack` / `table.unpack`. Everything else (`io`, `os`, `require`, `coroutine`, `debug`) is unavailable in v0.1.

`print(...)` is rebound to the spec's print syscall — it writes to the emulator console (stdout from the binary's perspective), not to the voxel field. Variadic; uses `tostring` per arg. Useful for debugging.

`time() -> u64` returns the monotonic tick counter (frames since cart start). `rand() -> [0, 1)` is a deterministic PRNG. `data_get("name") -> string|nil` reads from a `__data NAME__` blob.

---

## 11. Patterns and idioms

A grab-bag of techniques the bundled carts demonstrate. Crib freely.

### Sub-tile animation

Game logic on a tile grid (4 lattice cells per tile, say) but smooth movement *between* tiles. Maintain `(tile_x, tile_z)` for logical state and `(anim_t, anim_dir)` for the motion in progress. Each tick advance `anim_t`; render the entity at `tile + anim_t * anim_dir`. When `anim_t` hits 1, commit the move and look for the next desired direction. Pacman does this for the player and ghosts.

### Sticky controls

Read input every tick into `desired_dir`; only commit it when the entity reaches a tile boundary AND can move in that direction. Otherwise continue in the current direction (or stop if blocked). Players feel responsive without spamming inputs.

### Two-state full-refresh + persist

A cart with a heavy backdrop (terrain, mountain, sky) plus light foreground objects: opt into `set_persist_buffer(true)` and stamp the backdrop once in `init()`. Each frame, only modify the cells your foreground occupies; clear the previous-frame foreground cells explicitly. You get the perf of persist mode without giving up animation. Demo cart uses this for the flashing beacon on the static terrain.

### State machines as `local` integers

```lua
local STATE_PLAYING, STATE_WON, STATE_LOST = 0, 1, 2
local game_state = STATE_PLAYING
```

Lua doesn't have a native enum type. A few module-scope locals do the job. Pacman uses this pattern for game state and ghost mode.

### Restart-after-game-over

Wrap your one-time setup in a `setup_world()` function that sets all module-scope state from scratch. Call it from `init()` and on a button press during the game-over state. Letting one button (`btnp(6)` is conventional) restart the cart is much friendlier than requiring the user to re-launch the binary.

### Random in Lua

`rand()` is `[0, 1)`. To get an integer in `[0, n)`: `math.floor(rand() * n)`. To pick from a list: `t[math.floor(rand() * #t) + 1]`. The PRNG is deterministic; if you want shuffled-each-run behavior, mix in `time()` or call `rand()` once during init for an offset.

### Audio on game events

Trigger `sfx()` from inside the event branch, not from `update()` polling. E.g. play the chomp sound the tick the player crosses into a pellet tile, not every tick the player is *on* a pellet tile (you'd retrigger every frame and it'd buzz).

### Pinning sound effects to a channel

If a sound should always interrupt itself when retriggered (UI click, demo-cart effect, weapon fire), pass an explicit channel: `sfx(0, 0)`. Without that, default `ch=-1` finds a free voice and the previous instance keeps playing alongside.

---

## 12. Where to look next

- **[`SPEC.md`](SPEC.md)** — every syscall, the bit-for-bit data layouts, conformance contract.
- **[`CART_FORMAT.md`](CART_FORMAT.md)** — every section, validation rules, hex encodings.
- **`carts/` in the source tree** — six bundled carts cover the practical patterns. In rough order of complexity:
  - `beep.omni` — minimal SFX trigger.
  - `effects.omni` — eight SFX, one per per-step effect.
  - `inst.omni` — custom-instrument trick.
  - `jukebox.omni` — multiple songs + cross-fades + sfx-over-music.
  - `demo.omni` — procedural terrain via `vox_set` + `set_persist_buffer`.
  - `pacman.omni` — full game: maze, sprites, text HUD, sub-tile animation, AI behaviors, game states.

If you write a cart and find a rough edge — missing syscall, awkward limitation, idiom that's painful to express — that's useful feedback. v0.1 is pre-stable on purpose.
