# Omnivixion `.omni` Cart Format — v0.1

Status: working draft, sibling to `SPEC.md`. v0.x is pre-stable; carts may break between minors.

`.omni` is the single-file text-format that omnivixion carts are distributed as. Companion to `SPEC.md` — `SPEC.md` defines the console; this document defines what carts *look like* on disk.

## 0. Design rules

- **Human-inspectable.** Open in any editor. Diff in git. Paste in a forum.
- **One file per cart.** Sections are delimited by `__name__` markers, not separate files.
- **Forward-compatible.** Unknown sections and unknown TOML keys are warned-and-ignored, not rejected.
- **No build step required.** The emulator parses `.omni` directly. A future bundler can ship a compressed binary form for size-constrained contexts; the text format remains canonical.

## 1. File structure

```
omnivixion cart v0.1
# free-form comment lines allowed before first __section__

__header__
...TOML body...

__lua__
...Lua source...

__palette__
...

__sprites__
...

__data NAME__
...base64 blob...

__wasm__
...base64...   # only when header.runtime.lang == "wasm"

__source__     # optional, accompanies __wasm__
...freeform...
```

### 1.1 Magic line

The first non-empty line of the file MUST be:

```
omnivixion cart vMAJOR.MINOR
```

The emulator rejects carts with an unknown major. Lines before the magic line are not allowed. Trailing content on the magic line (after a space) is reserved.

### 1.2 Section markers

A section marker is a line whose entire content is `__name__` or `__name arg__`:

- `name` matches `[a-z][a-z0-9_]*`. The leading and trailing `__` are required.
- `arg` (optional) is a single token matching the same identifier rule. Used by sections that repeat (e.g. `__data sounds__`, `__data level1__`).
- The marker line is the section's separator; it's not part of the body.
- A section's body runs from the line after its marker up to (but not including) the next marker line, or end-of-file.

Whitespace before/after the marker tokens is not permitted (`__ header __` is not a section marker; it's a body line). Body lines that *happen* to start with `__` and look like a marker but don't match the grammar above are body content.

### 1.3 Section ordering

- `__header__` MUST be the first section. The emulator parses it before allocating anything for the rest of the cart so it can reject an incompatible cart cheaply.
- All other sections may appear in any order.
- Sections that are not allowed to repeat: `__header__`, `__lua__`, `__wasm__`, `__source__`. The loader rejects duplicates.
- Sections that may repeat (at most once per `arg`): `__palette__`, `__sprites__`, `__data ARG__`. Bodies are concatenated in source order; for `__palette__` repeats, later overrides win.

### 1.4 Comments and whitespace

- Outside `__lua__` and `__wasm__`, lines starting with `#` (after optional leading whitespace) are comments.
- Inside `__lua__`, `--` introduces a Lua comment per Lua syntax.
- `__wasm__` and `__data NAME__` bodies are base64; whitespace between base64 chars is ignored on decode.
- Both `LF` and `CRLF` line endings are accepted on read.
- Encoding is UTF-8.

### 1.5 Size limits

- Hard cap: uncompressed cart size 4 MiB. Anything larger is rejected at load.
- The spec's 512 KB compressed-cart budget (`SPEC.md` §5) is not enforced on `.omni` text carts in v0.1; it's a v0.2 concern when the bundled/compressed form is defined.

## 2. `__header__`

Body is TOML. The full v0.1 schema:

```toml
[meta]
spec     = "0.1"            # required. "MAJOR.MINOR". Major must match emulator.
name     = "Pacman"          # required. ≤ 32 chars. Cart picker label.
author   = "seth"            # required. ≤ 32 chars.
desc     = "voxel pacman"    # optional. ≤ 120 chars.
license  = "CC0"             # optional. SPDX id or freeform.
homepage = ""                # optional. URL.

[runtime]
lang = "lua"                 # "lua" | "wasm". Default "lua".
hz   = 30                    # 30 | 60. Default 30.

[display]
default_pitch  = 30          # 0..90. Default 30. Sets initial cam_pitch.
persist_buffer = false       # initial value of the persist flag. Default false.

[palette]
default = 0                  # 0 = built-in default palette. Reserved for v0.2 palette banks.

[audio]
banks = 0                    # reserved for v0.2; ignored in v0.1.
```

### Validation rules

- Missing required fields (`meta.spec`, `meta.name`, `meta.author`) → reject with line-numbered error.
- `meta.spec` major mismatch → reject.
- `runtime.lang` ∉ {`"lua"`, `"wasm"`} → reject.
- `runtime.hz` ∉ {30, 60} → reject.
- `display.default_pitch` outside `[0, 90]` → reject.
- `palette.default` not 0 in v0.1 → warn, treat as 0 (palette banks are v0.2).
- Unknown top-level table → warn, ignore (forward-compat).
- Unknown key inside a known table → warn, ignore (forward-compat).
- Excess length on `meta.name` / `meta.author` / `meta.desc` → reject (carts that don't fit the picker UI are explicitly out of spec).

A cart with `runtime.lang = "wasm"` is accepted by the format but rejected by the v0.1 loader with the message `"WASM runtime is reserved for v0.2"`. The format reserves the slot now so v0.2 doesn't need a flag day.

## 3. `__lua__`

Plain Lua 5.4 source. Required when `runtime.lang = "lua"`. Empty section is legal but silly.

The Lua chunk runs once at load time. Top-level `function init() ... end` and `function update(dt) ... end` definitions are picked up by the emulator and called as the cart's lifecycle hooks. Both are optional; a cart with neither is loaded but does nothing.

### Sandbox

The Lua environment exposes:

- All omnivixion syscalls listed in `SPEC.md` §6, plus the additions documented there (sprites, text drawing, `data_get`).
- The Lua stdlib subset: `string`, `table`, `math`, `pairs`, `ipairs`, `next`, `select`, `tostring`, `tonumber`, `type`, `error`, `assert`, `pcall`, `xpcall`, `unpack` / `table.unpack`.
- `print` is rebound to the spec's `print` syscall (writes to the emulator console, not stdout).

The Lua environment does NOT expose: `require`, `dofile`, `loadfile`, `load`, `loadstring`, `package`, `io`, `os`, `debug`, `coroutine` (reserved; v0.2 may add it back), `collectgarbage`. Carts must not reach outside the syscall surface.

### Syscall name mapping

Syscalls are exposed as Lua globals with the same names as in `SPEC.md` §6 / `CartApi`. For the trait methods that take a `TextOrient` enum, Lua passes a string instead:

| Rust enum variant | Lua string |
|---|---|
| `TextOrient::XYWall` | `"xy_wall"` |
| `TextOrient::XZFloor` | `"xz_floor"` |
| `TextOrient::ZYWall` | `"zy_wall"` |

So a Lua cart writes:

```lua
text_draw_axis("HELLO", 16, 4, 30, 10, "xz_floor")
```

`neighbor(x, y, z, idx)` returns three values (Lua multi-return): `local nx, ny, nz = neighbor(x, y, z, 0)`.

`data_get(name)` returns the data section's bytes as a Lua string, or `nil` if the section is absent.

## 4. `__palette__`

Each non-comment line is `INDEX: RRGGBB`:

- `INDEX` is a decimal integer in `[0, 15]`.
- `RRGGBB` is a 6-digit hex RGB color (no leading `#`).
- Index 0 is the empty-cell sentinel; lines with index 0 are tolerated but ignored.
- Indices 1–15 override the default palette at those slots.
- Skipped indices keep the default-palette value.
- Whitespace around `:` is permitted.
- A line that doesn't match the grammar is rejected with a line number.

```
__palette__
# index : hex
0: 000000   # empty sentinel — ignored
1: 1a1c2c
2: 5d275d
15: f4f4f4
```

If `__palette__` appears multiple times, later sections override earlier ones at conflicting indices. v0.2 reserves named palette banks via `__palette NAME__`; in v0.1 the loader warns and ignores unknown bank names.

## 5. `__sprites__`

Voxel sprite bank. Each sprite is a header line followed by one or more Y-slice grids.

### 5.1 Sprite header line

```
sprite ID NAME SX SY SZ
```

- `ID`: integer in `[0, 255]`. Maps directly to the sprite slot used by `spr_draw(id, x, y, z)`.
- `NAME`: identifier matching `[a-z][a-z0-9_]*`, ≤ 32 chars. Documentation only in v0.1; reserved for a future name-based syscall.
- `SX`, `SY`, `SZ`: voxel extents along each axis. The current `spr_load` syscall accepts only cubic sprites of size 2, 4, 8, or 16 — so `SX = SY = SZ` and the value must be one of those four. The format permits non-cubic sprites for forward-compat with v0.2 sprite ABI changes; the v0.1 loader rejects non-cubic and unsupported sizes with a clear error.

### 5.2 Y-slice grids

For each Y in `[0, SY)` that has any filled cells, a `y=N` line introduces the slice:

```
y=N
. 1 . 1 . 1 . 1
1 . 1 . 1 . 1 .
. 1 . 1 . 1 . 1
1 . 1 . 1 . 1 .
. 1 . 1 . 1 . 1
1 . 1 . 1 . 1 .
. 1 . 1 . 1 . 1
1 . 1 . 1 . 1 .
```

- Rows correspond to Z (top row is `z = 0`, last row is `z = SZ - 1`).
- Columns correspond to X (leftmost column is `x = 0`, rightmost is `x = SX - 1`).
- Tokens are whitespace-separated (any run of spaces or tabs).
- Each token is one glyph:
  - `.` — off-lattice cell. Required at every position where `(x + y + z)` is odd (parity-invalid). Writes are silently skipped per spec.
  - `0` — on-lattice empty cell. Allowed; useful for carving holes.
  - `1`–`9`, `a`–`f` — palette index. Lowercase only. (Uppercase A–F is rejected.)
- A sprite-local `(0, 0, 0)` is parity-even, so the `.` glyph appears at a checkerboard pattern. Putting a non-`.` glyph at an odd-parity position is rejected.
- Y slices that are missing (no `y=N` line for that N) are treated as fully empty (all-`.`/`0`).
- Y slices may appear in any order. Duplicate `y=N` lines for the same sprite are rejected.

### 5.3 Loader behavior

The loader packs each sprite's parity-even cells into the 4-bit-packed byte layout that the existing `spr_load` syscall expects (see `Console::sprite_cell_index` in `src/console.rs`). At cart init, before the user's Lua `init()` runs, the loader calls `spr_load(id, size, packed_bytes)` for each declared sprite.

### 5.4 Worked example

A single solid 4³ yellow sprite for the player:

```
__sprites__
sprite 3 player 4 4 4
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

(Palette index 1 here is just an example. Use the index that matches your intended color in the active palette.)

## 6. `__data NAME__`

A named base64 blob, accessible from the cart at runtime via `data_get("name")`.

- `NAME` must match `[a-z][a-z0-9_]*`, ≤ 32 chars.
- Body is a sequence of base64 characters (`A-Z`, `a-z`, `0-9`, `+`, `/`, `=`). Whitespace and `#` comment lines are ignored on decode.
- Standard base64 (RFC 4648, with `+`/`/` and `=` padding). URL-safe base64 (`-`/`_`) is not accepted in v0.1.
- Each `__data NAME__` section in the cart contributes one entry; duplicate names are rejected.

```
__data level1__
H4sIAAAAAAAAA+3UQU7DMBSF4X1OkfYC7QHaA8wmCq0SO1FNkE7QlSpFaiU
WhULZc8j8GJsm7gmlhtKFvNdJlsf28zd6jL7vR6ORztu2bdv7+8XiJ7vd7r
b9PB6P40+ej8eHGo3GwOC2bdv5fL4WCoVCWeyJI4Z+v9/v9z0eb7VanU5n
```

## 7. `__wasm__` (reserved, v0.2)

Single base64 blob carrying a WebAssembly module. Required when `runtime.lang = "wasm"`. Mutually exclusive with `__lua__` (a cart with both is rejected).

The v0.1 loader rejects any cart that selects the `wasm` runtime with a clear error. The format reserves the section so v0.2 doesn't break compatibility when the WASM ABI lands.

```
__wasm__
AGFzbQEAAAABBwFgAn9/AX8...
```

## 8. `__source__` (optional, accompanies `__wasm__`)

Plain text source for a `__wasm__` cart, for hackability. Not parsed by the loader; ignored at runtime. Encouraged for any cart distributed in WASM form.

```
__source__
# Rust 1.78, omnivixion-cart-rs 0.1
fn init(api: &mut CartApi) { ... }
fn update(api: &mut CartApi, dt: f32) { ... }
```

## 9. Forward compatibility

- Unknown sections (`__foo__`, `__foo bar__`) are warned-and-ignored at load.
- Unknown TOML tables and keys in `__header__` are warned-and-ignored.
- v0.x → v0.x+1 carts may break. v0.x → v(x+1).0 will require migration; the magic line's major is the gate.

## 10. End-to-end example

A minimal "spinning cube" cart that fills a 4³ region near the world center, using only on-the-fly `vox_set`:

```
omnivixion cart v0.1

__header__
[meta]
spec   = "0.1"
name   = "spinner"
author = "seth"
desc   = "smallest sensible cart"

[runtime]
lang = "lua"
hz   = 30

[display]
default_pitch  = 30
persist_buffer = false

__lua__
local n = 64

function update(dt)
  local t = time()
  for dx = 0, 3 do
    for dy = 0, 3 do
      for dz = 0, 3 do
        if (dx + dy + dz) % 2 == 0 then
          local color = 1 + ((t + dx + dy + dz) % 15)
          vox_set(n + dx, n + dy, n + dz, color)
        end
      end
    end
  end
end
```

No `__sprites__`, no `__palette__`, no `__data__` — none are required. The default palette and an empty sprite bank are used. `init()` is not declared; the loader skips it.
