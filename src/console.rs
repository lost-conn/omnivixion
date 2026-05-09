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

pub struct Console {
    /// Display buffer: 4 bits per valid cell, 2 cells per byte. Length = `VALID_CELLS / 2`
    /// = 524,288 bytes (512 KB), matching the spec.
    buffer: Vec<u8>,
    /// Per-cell count of filled face-neighbors (0..=12). One byte per valid cell.
    filled_neighbors: Vec<u8>,
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
        Self {
            buffer: vec![0u8; BUFFER_BYTES],
            filled_neighbors: vec![0u8; VALID_CELLS],
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
