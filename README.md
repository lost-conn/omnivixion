# omnivixion

A voxel fantasy console. The display is a 128³ rhombic-dodecahedral (FCC) lattice — instead of a 2D framebuffer, carts write into a small 3D grid of voxel cells, and the emulator decides how to render that grid.

The pitch is in two parts:

1. **Console ≠ display.** A cart's output is voxel positions and colors. How those get rendered (3D RDs, spheres, glowing points, slices, ASCII…) is the emulator's choice. Carts don't pick their look; the platform does.
2. **The lattice itself is the constraint.** No cubic grids — every cell is a rhombic dodecahedron, every move from one cell to another is a face-share with one of 12 face-neighbors. There's no axis-aligned step at lattice resolution; movement either zigzags or stays at the cubic-tile abstraction. That's a feature.

This is **early.** The core emulator works, the spec is at v0.1, several carts ship in `carts/` as `.omni` text files — a playable pacman, a procedural landscape, and a handful of audio demos — all loaded at startup via the Lua runtime. It's not a product yet.

For a hands-on guide to authoring carts, see [`MANUAL.md`](MANUAL.md). For the formal spec, see [`SPEC.md`](SPEC.md). For the cart file format, see [`CART_FORMAT.md`](CART_FORMAT.md).

---

## Quick start

Requires Rust (stable, edition 2024). Tested on Linux + Wayland with Vulkan.

```sh
cargo run --release                              # pacman cart (default)
cargo run --release -- --cart carts/demo.omni    # landscape demo (mountain, trees, sky tower)
cargo run --release -- --cart path/to/your.omni  # any other .omni cart
```

Cart paths are resolved relative to the working directory. `cargo run` runs from the crate root, so the bundled `carts/` is found automatically; for an installed binary, run from a directory that contains `carts/` or pass an absolute path to `--cart`.

Pacman controls:

| Key | Action |
|---|---|
| W A S D | Move (sticky — keeps going until a wall) |
| Z | Restart on game-over |
| Mouse drag | Orbit camera (dev) |
| Wheel | Zoom |

The maze scatter→chase phases, four ghost personalities (Blinky / Pinky / Inky / Clyde), power pellets, frightened mode, chained scoring, and lives all come from the cart code in `carts/pacman.omni` — no engine support beyond the spec's syscalls.

---

## What's in the box

- **`src/`** — Rust emulator.
  - `lattice.rs` — FCC math, parity rules, 12 face-neighbors, RD mesh.
  - `console.rs` — display buffer (512 KB, nibble-packed), sprite bank, palette, full `CartApi` surface.
  - `font.rs` — built-in 5×7 voxel font (full printable ASCII).
  - `render.rs` + `shader.wgsl` — wgpu pipeline: instanced RD draw, surface culling, distance fog.
  - `synth.rs` — cpal-backed 4-voice chiptune mixer (8 waveforms, per-step effects, custom instruments, music sequencer).
  - `loader.rs` — `.omni` cart parser + sandboxed Lua runtime via `mlua`.
  - `cart.rs` — `Cart` trait. Implementations come from `loader.rs` (`LuaCart`).
  - `main.rs` — winit + camera + 60 Hz logic / vsynced render.
- **`carts/`** — the bundled carts.
  - `pacman.omni` — voxel pacman (graphics).
  - `demo.omni` — procedural landscape showcase (graphics).
  - `beep.omni` — minimal audio smoke test.
  - `effects.omni` — per-step audio effects, one per button.
  - `inst.omni` — custom-instrument trick (SFX-as-voice).
  - `jukebox.omni` — music sequencer + cross-fade between songs.
- **`SPEC.md`** — the console spec, v0.1 draft.
- **`CART_FORMAT.md`** — the `.omni` cart text-format reference.
- **`MANUAL.md`** — cart-authoring guide.
- **`Cargo.toml` / `Cargo.lock`** — single crate.

---

## Spec highlights

(Full details in [`SPEC.md`](SPEC.md). Numbers are committed for v0.x; expect adjustments before stable.)

- **Lattice:** rhombic-dodecahedral, 128³ extent, 1,048,576 valid cells. Cells exist where `(x + y + z) mod 2 == 0`.
- **Display buffer:** 4 bits per cell, 2 cells per byte → exactly **512 KB**.
- **Color:** 16-color palette. Slot 0 = empty.
- **Cart memory:** 512 KB compressed (binary + assets), 256 KB working RAM, Lua-first cart runtime (WASM v0.2). Carts ship as `.omni` text files (see [`CART_FORMAT.md`](CART_FORMAT.md)); the Lua runtime is wired via `mlua`.
- **Camera:** one parameter (pitch ∈ [0, 90]°). Yaw is fixed in v0.1. Projection is the emulator's choice (perspective recommended for the lattice's geometry to read).
- **Update model:** **full-refresh by default** — the emulator clears the buffer before each `update()` so carts redraw statelessly. Carts with mostly-static scenes can `set_persist_buffer(true)` to keep state between ticks.
- **Sprites:** cubic only, sizes ∈ {2, 4, 8, 16}, 4 bits per cell, even-parity relative positions. 256-slot sprite bank.
- **Text:** built-in 5×7 font, 3 orientations (`XYWall` / `XZFloor` / `ZYWall`). Glyphs auto-snap z (or x or y, depending on orientation) by ±1 to satisfy lattice parity, so text is 1–2 cells thick along the depth axis.
- **Audio:** PICO-8-shaped 4-voice chiptune synth. 8 base waveforms (triangle / saw / square / pulse / organ / noise / phaser / tilted-saw), 8 per-step effects (slide / vibrato / drop / fades / arpeggios), 64 SFX × 32 steps, 128 patterns for music. Custom-instrument trick (SFX 0..7 usable as voices in other SFX). Output via `cpal`; emulator targets 22050 Hz mono floor and may upsample/stereoize.

---

## Why a rhombic-dodecahedral lattice?

Because cubes are everywhere and constraint = creativity. The RD lattice has:

- **One canonical neighborhood.** 12 face-neighbors per cell, all at distance √2. No 6/18/26 ambiguity.
- **No axis-aligned step.** Pure +X movement at lattice resolution doesn't exist; you either chain face-neighbor offsets that net to an axis (zigzag) or work at a coarser tile abstraction. Pacman's cart picks the latter for the maze grid and the former for inter-tile animation.
- **Sphere-packing aesthetic.** FCC is the densest sphere packing; RDs are its Voronoi cells. Things look organic.

An early TypeScript prototype (since deleted from the tree) tested both RD and the truncated-octahedral (BCC) alternative. RD won on aesthetic distinctiveness.

---

## Status / roadmap

What works today:

- Native Rust emulator on wgpu (Vulkan / Metal / DX12 / OpenGL via wgpu's backend choice).
- Full RD lattice math, parity-packed display buffer, surface-culled instanced rendering.
- Sprites (load + draw + clear), 5×7 text in three orientations, palette ops, input (10 buttons), camera pitch, RNG, time, restart.
- 4-voice chiptune audio via cpal: 8 waveforms, 8 effects, 64-slot SFX bank, 128-pattern music sequencer with cross-fades, PICO-8-style custom-instrument trick.
- A proof-of-platform game (pacman), a proof-of-canvas showcase (landscape demo), and four audio demo carts (beep, effects, inst, jukebox).

What's missing (decisions deferred or implementation pending):

- Cart binary header + compression (deferred to v0.2).
- WASM cart runtime alongside Lua.
- A sprite editor / asset pipeline. Right now sprites are hand-authored as text glyph grids in `.omni` `__sprites__` blocks (or built procedurally in Lua); no PNG→cart importer, no visual editor.
- A tracker / sound editor. SFX and music are hand-edited as hex strings in `__sfx__` / `__music__` blocks today.
- Default 15-color palette (currently a working set; not yet locked).
- Browser build (wgpu compiles to WebGPU; haven't wired the WASM emulator target).

---

## A note on the layout

This repo isn't yet "user-facing" — there's no installer, no published binary, no website. It's a working bench for the spec. If you want to play, build from source. If you want to contribute, the spec is the right starting point.
