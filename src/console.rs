//! Console state — display buffer, palette, input, time, RNG.
//! Implements `CartApi` (the syscall surface).

use std::collections::{HashMap, HashSet};
use winit::keyboard::KeyCode;

use crate::lattice::{cell_index, is_valid, NEIGHBOR_OFFSETS, N, VALID_CELLS};

/// Display-buffer byte count: 4 bits per cell, 2 cells per byte.
const BUFFER_BYTES: usize = VALID_CELLS / 2;

pub type Rgb = [f32; 3];

/// Default 15-color palette (slot 0 reserved for empty).
pub const DEFAULT_PALETTE: [Rgb; 16] = [
    [0.00, 0.00, 0.00], //  0 empty
    [0.10, 0.18, 0.45], //  1 deep blue
    [0.30, 0.55, 0.85], //  2 blue (water)
    [0.85, 0.78, 0.55], //  3 sand
    [0.42, 0.66, 0.30], //  4 grass
    [0.55, 0.62, 0.25], //  5 olive
    [0.18, 0.40, 0.20], //  6 forest
    [0.45, 0.32, 0.20], //  7 brown
    [0.55, 0.55, 0.58], //  8 stone
    [0.30, 0.30, 0.32], //  9 dark stone
    [0.92, 0.94, 0.96], // 10 snow
    [1.00, 0.86, 0.40], // 11 yellow
    [0.95, 0.55, 0.20], // 12 orange
    [0.85, 0.25, 0.20], // 13 red
    [0.85, 0.45, 0.65], // 14 pink
    [0.55, 0.30, 0.80], // 15 purple
];

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub pos: [f32; 3],
    pub _pad0: f32,
    pub color: [f32; 3],
    pub _pad1: f32,
}

/// A sprite — cubic voxel pattern, 4-bit cells, nibble-packed, even-parity relative
/// positions only. Color 0 = transparent.
struct Sprite {
    size: u8,
    /// `size³ / 4` bytes. Indexed by relative-cell compact index, same packing
    /// convention as the display buffer.
    data: Vec<u8>,
}

const SPRITE_SLOTS: usize = 256;

#[inline]
fn sprite_data_len(size: u8) -> Option<usize> {
    match size {
        2 | 4 | 8 | 16 => Some((size as usize).pow(3) / 4),
        _ => None,
    }
}

/// Orientation for text drawn into the voxel field.
///
/// Each orientation defines two perpendicular axes (advance + glyph-height) and
/// implicitly the depth axis used for parity z-snap. The depth axis is the one
/// orthogonal to the visible 2D glyph — invisible from the natural viewing
/// direction of that orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOrient {
    /// Default. Advance +X, glyph height +Y. Snap on Z. (Billboard / wall facing the camera.)
    XYWall,
    /// Advance +X, glyph height -Z. Snap on Y. (Text lying flat on a horizontal plane,
    /// readable from above with default yaw.)
    XZFloor,
    /// Advance +Z, glyph height +Y. Snap on X. (Side wall facing +X.)
    ZYWall,
}

#[derive(Clone, Copy)]
struct OrientVecs {
    right: [i32; 3], // glyph advance direction
    up: [i32; 3],    // glyph height direction (top of glyph at +up)
    snap_axis: usize, // 0=x, 1=y, 2=z — axis to nudge by +1 for parity
}

fn orient_vecs(o: TextOrient) -> OrientVecs {
    match o {
        TextOrient::XYWall => OrientVecs {
            right: [1, 0, 0],
            up: [0, 1, 0],
            snap_axis: 2,
        },
        TextOrient::XZFloor => OrientVecs {
            right: [1, 0, 0],
            up: [0, 0, -1],
            snap_axis: 1,
        },
        TextOrient::ZYWall => OrientVecs {
            right: [0, 0, 1],
            up: [0, 1, 0],
            snap_axis: 0,
        },
    }
}

/// Compact index for a sprite-local cell. Same scheme as the display buffer:
/// `z * size * (size/2) + y * (size/2) + (x >> 1)`. Sprite cells are valid iff
/// `(x + y + z)` is even (lattice parity).
#[inline]
fn sprite_cell_index(size: u8, x: u8, y: u8, z: u8) -> usize {
    let n = size as usize;
    let half = n / 2;
    (z as usize) * n * half + (y as usize) * half + ((x as usize) >> 1)
}

pub struct Console {
    /// Display buffer: 4 bits per valid cell, 2 cells per byte. Length = `VALID_CELLS / 2`
    /// = 524,288 bytes (512 KB), matching the spec.
    buffer: Vec<u8>,
    /// Per-cell count of filled face-neighbors (0..=12). One byte per valid cell.
    filled_neighbors: Vec<u8>,
    /// Sprite bank, 256 slots. Each slot is `None` until `spr_load` is called.
    sprites: Vec<Option<Sprite>>,
    /// When false (default), the emulator clears the display buffer before each
    /// `cart.update()` call so the cart can render statelessly. Carts with
    /// largely-static scenes (like the demo landscape) call `set_persist_buffer(true)`
    /// in `init` to opt back into the old persistent-buffer behavior.
    pub persist_buffer: bool,
    pub palette: [Rgb; 16],

    pub instances: Vec<Instance>,
    cell_to_slot: HashMap<u32, u32>,
    slot_to_cell: Vec<u32>,
    pub instances_dirty: bool,

    pub pitch: f32,
    pub keys_down: HashSet<KeyCode>,
    pub keys_pressed: HashSet<KeyCode>,
    pub tick: u64,
    rng_state: u64,
}

impl Console {
    pub fn new() -> Self {
        let mut sprites = Vec::with_capacity(SPRITE_SLOTS);
        sprites.resize_with(SPRITE_SLOTS, || None);
        Self {
            buffer: vec![0u8; BUFFER_BYTES],
            filled_neighbors: vec![0u8; VALID_CELLS],
            sprites,
            persist_buffer: false,
            palette: DEFAULT_PALETTE,
            instances: Vec::with_capacity(1 << 18),
            cell_to_slot: HashMap::with_capacity(1 << 18),
            slot_to_cell: Vec::with_capacity(1 << 18),
            instances_dirty: false,
            pitch: 30.0,
            keys_down: HashSet::new(),
            keys_pressed: HashSet::new(),
            tick: 0,
            rng_state: 0x9E37_79B9_7F4A_7C15,
        }
    }

    #[inline]
    fn read_cell(&self, idx: usize) -> u8 {
        let byte = self.buffer[idx >> 1];
        if (idx & 1) == 0 { byte & 0x0F } else { byte >> 4 }
    }

    #[inline]
    fn write_cell(&mut self, idx: usize, val: u8) {
        let byte_idx = idx >> 1;
        let v = val & 0x0F;
        let byte = self.buffer[byte_idx];
        self.buffer[byte_idx] = if (idx & 1) == 0 {
            (byte & 0xF0) | v
        } else {
            (byte & 0x0F) | (v << 4)
        };
    }

    fn alloc_slot(&mut self, idx: u32, x: i32, y: i32, z: i32, color: u8) {
        let slot = self.instances.len() as u32;
        self.cell_to_slot.insert(idx, slot);
        self.slot_to_cell.push(idx);
        self.instances.push(Instance {
            pos: [x as f32, y as f32, z as f32],
            _pad0: 0.0,
            color: self.palette[color as usize],
            _pad1: 0.0,
        });
        self.instances_dirty = true;
    }

    fn free_slot(&mut self, idx: u32) {
        let Some(slot) = self.cell_to_slot.remove(&idx) else {
            return;
        };
        let last = (self.instances.len() - 1) as u32;
        if slot != last {
            let moved_idx = self.slot_to_cell[last as usize];
            self.slot_to_cell[slot as usize] = moved_idx;
            self.cell_to_slot.insert(moved_idx, slot);
            self.instances[slot as usize] = self.instances[last as usize];
        }
        self.slot_to_cell.pop();
        self.instances.pop();
        self.instances_dirty = true;
    }

    /// Recompute whether the cell at (x, y, z) should have an instance and reconcile.
    /// Visible iff the cell is filled AND not fully buried.
    fn update_visibility(&mut self, idx: u32, x: i32, y: i32, z: i32) {
        let i = idx as usize;
        let color = self.read_cell(i);
        let buried = self.filled_neighbors[i] >= 12;
        let should_render = color != 0 && !buried;
        let has_slot = self.cell_to_slot.contains_key(&idx);
        match (should_render, has_slot) {
            (true, false) => self.alloc_slot(idx, x, y, z, color),
            (false, true) => self.free_slot(idx),
            (true, true) => {
                let slot = self.cell_to_slot[&idx] as usize;
                self.instances[slot].color = self.palette[color as usize];
                self.instances_dirty = true;
            }
            (false, false) => {}
        }
    }

    fn rand_u64(&mut self) -> u64 {
        // SplitMix64
        self.rng_state = self.rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

pub trait CartApi {
    fn vox_set(&mut self, x: i32, y: i32, z: i32, color: u8);
    fn vox_get(&self, x: i32, y: i32, z: i32) -> u8;
    fn vox_clear(&mut self);
    fn vox_fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, color: u8);
    fn vox_is_valid(&self, x: i32, y: i32, z: i32) -> bool;
    fn neighbor(&self, x: i32, y: i32, z: i32, idx: u8) -> (i32, i32, i32);

    /// Register a cubic sprite. `size` must be 4, 8, or 16.
    /// `data` length must be `size³ / 4` bytes (4-bit cells, nibble-packed,
    /// even-parity relative positions only). Returns true on success.
    fn spr_load(&mut self, id: u8, size: u8, data: &[u8]) -> bool;
    /// Stamp a sprite into the display buffer at world cell (x, y, z).
    /// Color 0 in the sprite is transparent. Anchor is parity-checked; out-of-grid
    /// cells are silently skipped.
    fn spr_draw(&mut self, id: u8, x: i32, y: i32, z: i32);
    /// Drop a sprite from the bank. After this, `spr_draw(id, ...)` is a no-op.
    fn spr_clear(&mut self, id: u8);

    /// Toggle persistent-buffer mode. When `false` (default), the emulator
    /// clears the display buffer at the start of every `update()` so the cart
    /// can render statelessly. When `true`, the buffer carries between frames
    /// and the cart is responsible for clearing what it wants gone.
    fn set_persist_buffer(&mut self, persist: bool);
    /// Read the current persistent-buffer flag.
    fn persist_buffer(&self) -> bool;

    /// Stamp a string of glyphs in the default `XYWall` orientation
    /// (advance +X, glyph height +Y, z-snap for parity).
    fn text_draw(&mut self, s: &str, x: i32, y: i32, z: i32, color: u8);
    /// Stamp a string in an explicit orientation. See `TextOrient`.
    fn text_draw_axis(&mut self, s: &str, x: i32, y: i32, z: i32, color: u8, orient: TextOrient);
    /// Advance per glyph along the orientation's `right` axis (5-cell glyph + 1-cell gap).
    fn text_advance(&self) -> u8;
    /// Glyph height in cells along the orientation's `up` axis.
    fn text_height(&self) -> u8;

    fn pal_set(&mut self, slot: u8, r: f32, g: f32, b: f32);
    fn pal_reset(&mut self);
    fn cam_pitch(&mut self, deg: f32);
    fn cam_pitch_get(&self) -> f32;
    fn btn(&self, idx: u8) -> bool;
    fn btnp(&self, idx: u8) -> bool;
    fn time(&self) -> u64;
    fn rand(&mut self) -> f32;
    fn print(&self, msg: &str);
}

fn btn_keys(idx: u8) -> &'static [KeyCode] {
    match idx {
        0 => &[KeyCode::KeyA],
        1 => &[KeyCode::KeyD],
        2 => &[KeyCode::KeyS],
        3 => &[KeyCode::KeyW],
        4 => &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
        5 => &[KeyCode::Space],
        6 => &[KeyCode::KeyZ],
        7 => &[KeyCode::KeyX],
        8 => &[KeyCode::KeyC],
        9 => &[KeyCode::KeyV],
        _ => &[],
    }
}

impl CartApi for Console {
    fn vox_set(&mut self, x: i32, y: i32, z: i32, color: u8) {
        if !is_valid(x, y, z) {
            return;
        }
        let color = color & 0x0f;
        let idx = cell_index(x, y, z);
        let prev = self.read_cell(idx);
        if prev == color {
            return;
        }
        let was_filled = prev != 0;
        let new_filled = color != 0;
        self.write_cell(idx, color);

        // If filled status flipped, propagate to the 12 face-neighbors so their
        // burial state is up to date, then reconcile each neighbor's visibility.
        if was_filled != new_filled {
            for offset in NEIGHBOR_OFFSETS.iter() {
                let nx = x + offset[0];
                let ny = y + offset[1];
                let nz = z + offset[2];
                if !is_valid(nx, ny, nz) {
                    continue;
                }
                let nidx = cell_index(nx, ny, nz);
                if new_filled {
                    self.filled_neighbors[nidx] = self.filled_neighbors[nidx].saturating_add(1);
                } else {
                    self.filled_neighbors[nidx] = self.filled_neighbors[nidx].saturating_sub(1);
                }
                self.update_visibility(nidx as u32, nx, ny, nz);
            }
        }

        // Reconcile this cell. Self's filled_neighbors didn't change above; only
        // its color/filled status did, so visibility may flip on/off or just recolor.
        self.update_visibility(idx as u32, x, y, z);
    }

    fn vox_get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !is_valid(x, y, z) {
            return 0;
        }
        self.read_cell(cell_index(x, y, z))
    }

    fn vox_clear(&mut self) {
        self.buffer.fill(0);
        self.filled_neighbors.fill(0);
        self.cell_to_slot.clear();
        self.slot_to_cell.clear();
        self.instances.clear();
        self.instances_dirty = true;
    }

    fn vox_fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, color: u8) {
        let lx = x0.min(x1).max(0);
        let ly = y0.min(y1).max(0);
        let lz = z0.min(z1).max(0);
        let ux = x0.max(x1).min(N - 1);
        let uy = y0.max(y1).min(N - 1);
        let uz = z0.max(z1).min(N - 1);
        for z in lz..=uz {
            for y in ly..=uy {
                for x in lx..=ux {
                    self.vox_set(x, y, z, color);
                }
            }
        }
    }

    fn vox_is_valid(&self, x: i32, y: i32, z: i32) -> bool {
        is_valid(x, y, z)
    }

    fn neighbor(&self, x: i32, y: i32, z: i32, idx: u8) -> (i32, i32, i32) {
        let o = NEIGHBOR_OFFSETS[(idx as usize).min(11)];
        (x + o[0], y + o[1], z + o[2])
    }

    fn spr_load(&mut self, id: u8, size: u8, data: &[u8]) -> bool {
        let Some(expected) = sprite_data_len(size) else { return false };
        if data.len() != expected {
            return false;
        }
        self.sprites[id as usize] = Some(Sprite { size, data: data.to_vec() });
        true
    }

    fn spr_draw(&mut self, id: u8, x: i32, y: i32, z: i32) {
        // Anchor must be parity-valid; we draw even-parity relative cells, so
        // (anchor.parity + relative.parity) must be even, i.e. anchor itself must
        // be even-sum. Anything else is a no-op for spec consistency.
        if !is_valid(x, y, z) {
            return;
        }
        // Take the sprite out of the bank for the duration of the draw so we can
        // freely call &mut self::vox_set in the inner loop. Returned at the end.
        let Some(spr) = self.sprites[id as usize].take() else { return };
        let size = spr.size as i32;

        for rz in 0..size {
            for ry in 0..size {
                // For (rx + ry + rz) to be even, rx parity must match (ry + rz) parity.
                let rx_start = (ry + rz) & 1;
                let mut rx = rx_start;
                while rx < size {
                    let rel_idx = sprite_cell_index(spr.size, rx as u8, ry as u8, rz as u8);
                    let byte = spr.data[rel_idx >> 1];
                    let color = if (rel_idx & 1) == 0 { byte & 0x0F } else { byte >> 4 };
                    if color != 0 {
                        self.vox_set(x + rx, y + ry, z + rz, color);
                    }
                    rx += 2;
                }
            }
        }

        self.sprites[id as usize] = Some(spr);
    }

    fn spr_clear(&mut self, id: u8) {
        self.sprites[id as usize] = None;
    }

    fn set_persist_buffer(&mut self, persist: bool) {
        self.persist_buffer = persist;
    }
    fn persist_buffer(&self) -> bool {
        self.persist_buffer
    }

    fn text_draw(&mut self, s: &str, x: i32, y: i32, z: i32, color: u8) {
        self.text_draw_axis(s, x, y, z, color, TextOrient::XYWall);
    }

    fn text_draw_axis(
        &mut self,
        s: &str,
        x: i32,
        y: i32,
        z: i32,
        color: u8,
        orient: TextOrient,
    ) {
        if !(1..=15).contains(&color) {
            return;
        }
        let v = orient_vecs(orient);
        let advance = crate::font::FONT_ADVANCE as i32;
        let h = crate::font::FONT_HEIGHT as i32;
        let w = crate::font::FONT_WIDTH as i32;

        let mut cur = [x, y, z];
        for c in s.chars() {
            let g = crate::font::glyph(c);
            for ry in 0..h {
                let row = g[ry as usize];
                let up_mul = h - 1 - ry; // ry=0 (top of glyph) → max up
                for rx in 0..w {
                    let on = (row >> (w - 1 - rx)) & 1 == 1;
                    if !on {
                        continue;
                    }
                    let mut wp = [
                        cur[0] + v.right[0] * rx + v.up[0] * up_mul,
                        cur[1] + v.right[1] * rx + v.up[1] * up_mul,
                        cur[2] + v.right[2] * rx + v.up[2] * up_mul,
                    ];
                    if !is_valid(wp[0], wp[1], wp[2]) {
                        wp[v.snap_axis] += 1;
                    }
                    self.vox_set(wp[0], wp[1], wp[2], color);
                }
            }
            cur[0] += v.right[0] * advance;
            cur[1] += v.right[1] * advance;
            cur[2] += v.right[2] * advance;
        }
    }

    fn text_advance(&self) -> u8 {
        crate::font::FONT_ADVANCE
    }

    fn text_height(&self) -> u8 {
        crate::font::FONT_HEIGHT
    }

    fn pal_set(&mut self, slot: u8, r: f32, g: f32, b: f32) {
        if !(1..=15).contains(&slot) {
            return;
        }
        self.palette[slot as usize] = [r, g, b];
        for i in 0..self.instances.len() {
            let cell_idx = self.slot_to_cell[i] as usize;
            if self.read_cell(cell_idx) == slot {
                self.instances[i].color = [r, g, b];
            }
        }
        self.instances_dirty = true;
    }

    fn pal_reset(&mut self) {
        for slot in 1..16u8 {
            let [r, g, b] = DEFAULT_PALETTE[slot as usize];
            self.pal_set(slot, r, g, b);
        }
    }

    fn cam_pitch(&mut self, deg: f32) {
        self.pitch = deg.clamp(0.0, 90.0);
    }
    fn cam_pitch_get(&self) -> f32 {
        self.pitch
    }

    fn btn(&self, idx: u8) -> bool {
        btn_keys(idx).iter().any(|k| self.keys_down.contains(k))
    }
    fn btnp(&self, idx: u8) -> bool {
        btn_keys(idx).iter().any(|k| self.keys_pressed.contains(k))
    }

    fn time(&self) -> u64 {
        self.tick
    }
    fn rand(&mut self) -> f32 {
        let bits = (self.rand_u64() >> 40) as u32;
        bits as f32 / (1u32 << 24) as f32
    }
    fn print(&self, msg: &str) {
        println!("[cart] {msg}");
    }
}
