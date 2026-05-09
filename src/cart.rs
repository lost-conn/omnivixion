//! Cart trait + the in-tree carts (DemoCart for the showcase scene, PacmanCart for the game).

use crate::console::{CartApi, TextOrient};

pub trait Cart {
    fn init(&mut self, api: &mut dyn CartApi);
    fn update(&mut self, api: &mut dyn CartApi, dt: f32);
}

// ============================================================================
//  DemoCart — landscape showcase (kept for reference; not used by default).
// ============================================================================

pub struct DemoCart {
    mountain_top_x: i32,
    mountain_top_z: i32,
    beacon_y: i32,
}

impl DemoCart {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self { mountain_top_x: 64, mountain_top_z: 64, beacon_y: 0 }
    }
}

const SEA_LEVEL: i32 = 16;

fn demo_height_at(x: i32, z: i32) -> i32 {
    let xf = x as f32;
    let zf = z as f32;
    let cx = xf - 64.0;
    let cz = zf - 64.0;
    let dist = (cx * cx + cz * cz).sqrt();

    let mut h = 18.0_f32;
    h += 11.0 * (xf * 0.062).sin() * (zf * 0.054).cos();
    h += 6.0 * ((xf + zf) * 0.13).sin();
    h += 4.0 * (xf * 0.21 - zf * 0.18).cos();
    h += 2.5 * (xf * 0.41).sin() * (zf * 0.39).cos();
    h += (38.0 - dist * 0.78).max(0.0);
    h += 6.0 * (-((dist - 50.0) / 14.0).powi(2)).exp();
    h.floor().clamp(2.0, 95.0) as i32
}

fn demo_color_for_elevation(y: i32, top: i32) -> u8 {
    if y <= SEA_LEVEL + 1 {
        if y == top { 3 } else { 7 }
    } else if y < 28 { 4 }
    else if y < 34 { 5 }
    else if y < 50 { 8 }
    else if y < 65 { 9 }
    else { 10 }
}

impl Cart for DemoCart {
    fn init(&mut self, api: &mut dyn CartApi) {
        api.print("Generating world...");
        let n = 128i32;
        for x in 0..n {
            for z in 0..n {
                let top = demo_height_at(x, z);
                for y in 0..=top {
                    if (x + y + z) & 1 != 0 { continue; }
                    api.vox_set(x, y, z, demo_color_for_elevation(y, top));
                }
            }
        }
        for x in 0..n {
            for z in 0..n {
                let top = demo_height_at(x, z);
                for y in (top + 1)..=SEA_LEVEL {
                    if (x + y + z) & 1 != 0 { continue; }
                    api.vox_set(x, y, z, 2);
                }
            }
        }
        for _ in 0..900 {
            let x = (api.rand() * n as f32) as i32;
            let z = (api.rand() * n as f32) as i32;
            let top = demo_height_at(x, z);
            if top < SEA_LEVEL + 2 || top > 32 { continue; }
            for dy in 1..=4 { api.vox_set(x, top + dy, z, 7); }
            for dx in -2..=2 {
                for dz in -2..=2 {
                    for dy in 3..=7 {
                        let cy = (dy - 5) as f32;
                        let r2 = (dx * dx + dz * dz) as f32 + cy * cy;
                        if r2 > 5.5 { continue; }
                        api.vox_set(x + dx, top + dy, z + dz, 6);
                    }
                }
            }
        }
        let mut peak_y = 0i32;
        for dx in -4..=4 {
            for dz in -4..=4 {
                let h = demo_height_at(64 + dx, 64 + dz);
                if h > peak_y {
                    peak_y = h;
                    self.mountain_top_x = 64 + dx;
                    self.mountain_top_z = 64 + dz;
                }
            }
        }
        let tower_height = 22i32;
        for dy in 1..=tower_height {
            let y = peak_y + dy;
            let radius = if dy < 4 { 3 } else { 2 };
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if dx * dx + dz * dz > radius * radius { continue; }
                    if dy >= tower_height - 4
                        && dx * dx + dz * dz < (radius - 1) * (radius - 1)
                    {
                        continue;
                    }
                    let cx = self.mountain_top_x + dx;
                    let cz = self.mountain_top_z + dz;
                    api.vox_set(cx, y, cz, if dy < 5 { 9 } else { 8 });
                }
            }
        }
        self.beacon_y = peak_y + tower_height + 2;
        api.vox_set(self.mountain_top_x, self.beacon_y, self.mountain_top_z, 13);
        api.print("done.");
    }

    fn update(&mut self, api: &mut dyn CartApi, _dt: f32) {
        let t = api.time();
        let lit = ((t / 30) & 1) == 0;
        api.vox_set(
            self.mountain_top_x,
            self.beacon_y,
            self.mountain_top_z,
            if lit { 13 } else { 11 },
        );
        if api.btnp(6) { api.cam_pitch((api.cam_pitch_get() + 10.0).min(90.0)); }
        if api.btnp(7) { api.cam_pitch((api.cam_pitch_get() - 10.0).max(0.0)); }
    }
}

// ============================================================================
//  Sprite-data helpers (cart-side authoring).
// ============================================================================

/// Build a 4³ sprite where each y-layer is filled with a single color.
/// Returns the 16-byte nibble-packed data the emulator expects.
#[allow(dead_code)]
fn make_layered_sprite_4(layers: [u8; 4]) -> [u8; 16] {
    let size: u8 = 4;
    let mut data = [0u8; 16];
    for rz in 0..size {
        for ry in 0..size {
            let mut rx = (ry + rz) & 1;
            while rx < size {
                let rel_idx = (rz as usize) * (size as usize) * (size as usize / 2)
                    + (ry as usize) * (size as usize / 2)
                    + ((rx as usize) >> 1);
                let color = layers[ry as usize] & 0x0F;
                let byte_idx = rel_idx >> 1;
                if rel_idx & 1 == 0 {
                    data[byte_idx] |= color;
                } else {
                    data[byte_idx] |= color << 4;
                }
                rx += 2;
            }
        }
    }
    data
}

/// Build a 4³ sprite filled solid with a single color across all 32 even-parity
/// cells. 16 bytes (32 nibbles), nibble-packed in the same layout as the
/// display buffer's compact index.
fn make_solid_sprite_4(color: u8) -> [u8; 16] {
    let c = color & 0x0F;
    let byte = c | (c << 4);
    [byte; 16]
}

/// Build a 2³ sprite filled solid with a single color (4 even-parity cells, 2 bytes).
fn make_solid_sprite_2(color: u8) -> [u8; 2] {
    let c = color & 0x0F;
    let byte = c | (c << 4);
    [byte; 2]
}

// ============================================================================
//  PacmanCart — voxel pacman.
// ============================================================================

const MAZE_SIZE: i32 = 16;
const STRIDE: i32 = 4;
const MAZE_X0: i32 = 32;
const MAZE_Z0: i32 = 32;
const GAME_Y: i32 = 2;

// Movement is animated as 4 sub-steps per tile move (each sub-step is one
// RD-neighbor offset; the chain of 4 nets to a cardinal stride). Tile-move
// periods MUST be multiples of SUB_STEPS so the per-sub-step frame budget is
// integer.
const SUB_STEPS:        u8  = 4;
const PLAYER_PERIOD:    u64 = 8;   // 2 frames / sub-step — full tile in 8 frames
const GHOST_PERIOD:     u64 = 20;  // Normal-mode ghost: 5 frames / sub-step
const FRIGHT_PERIOD:    u64 = 28;  // Frightened: 7 frames / sub-step (slower)
const EATEN_PERIOD:     u64 = 8;   // Eaten ghosts return to spawn fast (2 / sub-step)
const INTRO_GRACE:      u64 = 60;  // ghosts hold still for 1 second after game start
const FRIGHT_DURATION:  u64 = 360; // 6s of Frightened mode after a power pellet

// Title text. Anchored just in front of the maze (smaller-Z side) on the
// XZFloor plane so it reads from the default overhead camera.
const TITLE_X: i32 = 16;
const TITLE_Y: i32 = 4;
const TITLE_Z: i32 = 30;
const TITLE_MAX_CHARS: i32 = 16;

// HUD on a horizontal plane just to the right of the maze, XZFloor orientation.
// Stacked four lines in +Z (each line is 8 cells "below" the previous on screen
// at the default overhead camera).
const HUD_X: i32 = 96;
const HUD_Y: i32 = 6;
const HUD_Z_BASE: i32 = 36;
const HUD_MAX_CHARS: i32 = 5;

const LIVES_START: u8 = 3;
const RESPAWN_GRACE: u64 = 60;

const COLOR_WALL:   u8 = 8;   // stone
const COLOR_PELLET: u8 = 10;  // snow/white — small dots on the floor
const COLOR_PLAYER: u8 = 11;  // yellow — classic pacman
const COLOR_POWER:  u8 = 14;  // pink — power pellet
const COLOR_FRIGHT: u8 = 1;   // deep blue — Frightened ghost
const COLOR_EATEN:  u8 = 10;  // snow — Eaten ghost (just the eyes, conceptually)
// Ghost colors keyed to GhostBehavior order: Blinky red, Pinky pink, Inky blue, Clyde orange.
const COLOR_GHOST: [u8; 4] = [13, 14, 2, 12];

// Sprite bank IDs used by the pacman cart.
const SPR_WALL:       u8 = 0;
const SPR_POWER:      u8 = 2;  // 2³ pink blob; ID 1 left free for future floor
const SPR_PLAYER:     u8 = 3;
const SPR_GHOST_BASE: u8 = 4;  // 4..7 — one per ghost behavior
const SPR_FRIGHT:     u8 = 8;  // Frightened ghost (deep blue)
const SPR_EATEN:      u8 = 9;  // Eaten ghost (snow/white)

// Classic pacman scatter/chase schedule. Frame counts are 60 Hz.
const PHASE_SCHEDULE: &[(GamePhase, u64)] = &[
    (GamePhase::Scatter, 420),  // 7s
    (GamePhase::Chase,   1200), // 20s
    (GamePhase::Scatter, 420),
    (GamePhase::Chase,   1200),
    (GamePhase::Scatter, 300),  // 5s
    (GamePhase::Chase,   1200),
    (GamePhase::Scatter, 300),
    (GamePhase::Chase,   u64::MAX), // indefinite
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tile {
    Wall,
    Pellet,
    PowerPellet,
    Empty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameState {
    Playing,
    Won,
    Lost,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GhostBehavior {
    Blinky, // red, direct chase
    Pinky,  // pink, 4 tiles ahead of player heading
    Inky,   // blue, vector from Blinky through (player + 2 ahead)
    Clyde,  // orange, chase at distance, scatter when close
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Frightened/Eaten land in phase C
enum GhostMode {
    Normal,
    Frightened,
    Eaten,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GamePhase {
    Scatter,
    Chase,
}

/// In-flight tile-to-tile move. The entity's logical tile is `(src_tx, src_tz)`
/// until `sub_step` reaches `SUB_STEPS`, at which point we finalize the move.
/// Each sub-step is an RD-neighbor offset that nets `dir * 4` after 4 sub-steps.
#[derive(Clone, Copy, Debug)]
struct Anim {
    src_tx: i32,
    src_tz: i32,
    dir: (i32, i32), // tile-direction: ±x or ±z
    sub_step: u8,    // 0 = at src, advances on each sub-step boundary
    sub_frame: u64,  // frames into the current sub_step
}

struct Ghost {
    tx: i32,
    tz: i32,
    color: u8,
    last_dir: (i32, i32),
    behavior: GhostBehavior,
    mode: GhostMode,
    home_corner: (i32, i32),
    spawn: (i32, i32),
    anim: Option<Anim>,
}

pub struct PacmanCart {
    maze: Vec<Vec<Tile>>,
    player_tx: i32,
    player_tz: i32,
    /// Direction the player most recently committed to. Used for Pinky/Inky
    /// targeting and as the "keep going" fallback at tile boundaries.
    player_dir: (i32, i32),
    /// Last direction the user requested via input. At each tile boundary the
    /// player tries this first, then falls back to player_dir.
    desired_dir: (i32, i32),
    /// Animation state for the player's current tile-to-tile move; None when
    /// stopped at a tile (wall ahead and no valid input).
    player_anim: Option<Anim>,
    ghosts: Vec<Ghost>,
    pellets_remaining: u32,
    score: u32,
    lives: u8,
    state: GameState,
    phase: GamePhase,
    phase_timer: u64,
    phase_index: usize,
    /// Counts down while any ghosts are Frightened. Pauses phase_timer.
    frightened_timer: u64,
    /// Consecutive ghosts eaten in current Frightened spell.
    ghost_chain: u8,
    /// Tick threshold below which ghosts hold still after a respawn.
    respawn_until: u64,
    /// Last values we drew into the HUD; we only restamp on change.
    last_drawn_score: Option<u32>,
    last_drawn_lives: Option<u8>,
    rng: u64,
    announced_end: bool,
}

impl PacmanCart {
    pub fn new() -> Self {
        Self {
            maze: Vec::new(),
            player_tx: 0,
            player_tz: 0,
            player_dir: (0, 0),
            desired_dir: (0, 0),
            player_anim: None,
            ghosts: Vec::new(),
            pellets_remaining: 0,
            score: 0,
            lives: LIVES_START,
            state: GameState::Playing,
            phase: GamePhase::Scatter,
            phase_timer: 0,
            phase_index: 0,
            frightened_timer: 0,
            ghost_chain: 0,
            respawn_until: 0,
            last_drawn_score: None,
            last_drawn_lives: None,
            rng: 0xDEAD_BEEF_CAFE_BABE,
            announced_end: false,
        }
    }

    fn tile_to_world(tx: i32, tz: i32) -> (i32, i32) {
        (MAZE_X0 + STRIDE * tx, MAZE_Z0 + STRIDE * tz)
    }

    /// Stamp the static contents of a single maze tile (called both at world
    /// init and whenever an entity vacates a tile). Corridors are intentionally
    /// empty (dark gaps) so pellets and power pellets read clearly against the
    /// background instead of fighting a 4³ floor block for the eye's attention.
    ///
    /// Pellets sit at the geometric center of the 4³ tile region rather than
    /// at the anchor corner, so they line up with the centers of walls and
    /// entities (which are anchored at the same corner but fill the full tile).
    fn stamp_tile(&self, tx: i32, tz: i32, api: &mut dyn CartApi) {
        let (ax, az) = Self::tile_to_world(tx, tz);
        match self.maze[tx as usize][tz as usize] {
            Tile::Wall => api.spr_draw(SPR_WALL, ax, GAME_Y, az),
            Tile::Pellet => {
                // Single cell at the visual center of the 4³ tile region.
                api.vox_set(ax + 2, GAME_Y + 2, az + 2, COLOR_PELLET);
            }
            Tile::PowerPellet => {
                // 2³ sprite shifted +1 in x and z so its 2-cell span straddles
                // the tile's xz center instead of clinging to a corner.
                api.spr_draw(SPR_POWER, ax + 1, GAME_Y, az + 1);
            }
            Tile::Empty => {} // empty corridor — no cells
        }
    }

    /// Clear an entity sprite's 4³ bounding box at the given world anchor.
    fn clear_entity_box(ax: i32, ay: i32, az: i32, api: &mut dyn CartApi) {
        api.vox_fill(ax, ay, az, ax + 3, ay + 3, az + 3, 0);
    }

    /// World anchor for the entity's sprite given an animation state. Even
    /// sub-steps are at GAME_Y (ground); odd sub-steps bob 1 cell in y, which
    /// is the parity-preserving way to make a "1-cell" axis-aligned move
    /// visible on the FCC lattice.
    fn anim_anchor(anim: &Anim) -> (i32, i32, i32) {
        let (sx, sz) = Self::tile_to_world(anim.src_tx, anim.src_tz);
        let n = anim.sub_step as i32;
        let bob = (anim.sub_step & 1) as i32;
        (sx + n * anim.dir.0, GAME_Y + bob, sz + n * anim.dir.1)
    }

    /// Anchor for an entity sitting still on a tile.
    fn idle_anchor(tx: i32, tz: i32) -> (i32, i32, i32) {
        let (sx, sz) = Self::tile_to_world(tx, tz);
        (sx, GAME_Y, sz)
    }

    fn frames_per_sub_step_player(&self) -> u64 {
        PLAYER_PERIOD / SUB_STEPS as u64
    }
    fn frames_per_sub_step_ghost(mode: GhostMode) -> u64 {
        let period = match mode {
            GhostMode::Normal => GHOST_PERIOD,
            GhostMode::Frightened => FRIGHT_PERIOD,
            GhostMode::Eaten => EATEN_PERIOD,
        };
        period / SUB_STEPS as u64
    }

    /// Clear the title bbox and stamp `s` (≤ TITLE_MAX_CHARS) on a horizontal
    /// plane above the maze using the XZFloor orientation.
    fn draw_title(api: &mut dyn CartApi, s: &str, color: u8) {
        let advance = api.text_advance() as i32;
        let height = api.text_height() as i32;
        let max_w = TITLE_MAX_CHARS * advance;
        // XZFloor: text extends +X for advance, -Z for glyph height. Snap on Y.
        api.vox_fill(
            TITLE_X, TITLE_Y, TITLE_Z - height,
            TITLE_X + max_w, TITLE_Y + 1, TITLE_Z + 1,
            0,
        );
        api.text_draw_axis(s, TITLE_X, TITLE_Y, TITLE_Z, color, TextOrient::XZFloor);
    }

    /// Stamp one HUD line (XZFloor) at (HUD_X, HUD_Y, anchor_z) with the bbox
    /// cleared first so previous values are erased.
    fn draw_hud_line(api: &mut dyn CartApi, anchor_z: i32, s: &str, color: u8) {
        let advance = api.text_advance() as i32;
        let height = api.text_height() as i32;
        let max_w = HUD_MAX_CHARS * advance;
        // XZFloor: text extends +X for advance, -Z for glyph height. Snap on Y.
        api.vox_fill(
            HUD_X, HUD_Y, anchor_z - height,
            HUD_X + max_w, HUD_Y + 1, anchor_z + 1,
            0,
        );
        api.text_draw_axis(s, HUD_X, HUD_Y, anchor_z, color, TextOrient::XZFloor);
    }

    fn redraw_hud_if_dirty(&mut self, api: &mut dyn CartApi) {
        if self.last_drawn_score != Some(self.score) {
            if self.last_drawn_score.is_none() {
                Self::draw_hud_line(api, HUD_Z_BASE, "SCORE", COLOR_PELLET);
            }
            Self::draw_hud_line(api, HUD_Z_BASE + 8, &format!("{:05}", self.score), COLOR_PELLET);
            self.last_drawn_score = Some(self.score);
        }
        if self.last_drawn_lives != Some(self.lives) {
            if self.last_drawn_lives.is_none() {
                Self::draw_hud_line(api, HUD_Z_BASE + 16, "LIVES", COLOR_PLAYER);
            }
            Self::draw_hud_line(api, HUD_Z_BASE + 24, &format!("{}", self.lives), COLOR_PLAYER);
            self.last_drawn_lives = Some(self.lives);
        }
    }

    /// Move player + all ghosts back to their spawn tiles, drop any in-flight
    /// Frightened/Eaten state, and start a brief grace window before ghosts move
    /// again. Pellets and score persist.
    fn soft_reset(&mut self, t: u64, api: &mut dyn CartApi) {
        // Clear at the entity's current visual anchor (which may be mid-anim).
        let (px, py, pz) = self.player_anchor();
        Self::clear_entity_box(px, py, pz, api);
        self.stamp_tile(self.player_tx, self.player_tz, api);

        self.player_tx = MAZE_SIZE / 2;
        self.player_tz = MAZE_SIZE / 2;
        self.player_dir = (0, 0);
        self.desired_dir = (0, 0);
        self.player_anim = None;

        for gi in 0..self.ghosts.len() {
            let (gx, gy, gz) = self.ghost_anchor(gi);
            Self::clear_entity_box(gx, gy, gz, api);
            self.stamp_tile(self.ghosts[gi].tx, self.ghosts[gi].tz, api);

            let g = &mut self.ghosts[gi];
            g.tx = g.spawn.0;
            g.tz = g.spawn.1;
            g.last_dir = (0, 0);
            g.mode = GhostMode::Normal;
            g.anim = None;
        }

        self.frightened_timer = 0;
        self.ghost_chain = 0;
        self.respawn_until = t + RESPAWN_GRACE;

        // Re-render player and ghosts at their fresh positions.
        self.render_player(api);
        for gi in 0..self.ghosts.len() {
            self.render_ghost(gi, api);
        }
    }

    fn render_static_world(&self, api: &mut dyn CartApi) {
        for tx in 0..MAZE_SIZE {
            for tz in 0..MAZE_SIZE {
                self.stamp_tile(tx, tz, api);
            }
        }
    }

    fn player_anchor(&self) -> (i32, i32, i32) {
        match &self.player_anim {
            Some(anim) => Self::anim_anchor(anim),
            None => Self::idle_anchor(self.player_tx, self.player_tz),
        }
    }
    fn ghost_anchor(&self, gi: usize) -> (i32, i32, i32) {
        let g = &self.ghosts[gi];
        match &g.anim {
            Some(anim) => Self::anim_anchor(anim),
            None => Self::idle_anchor(g.tx, g.tz),
        }
    }

    fn render_player(&self, api: &mut dyn CartApi) {
        let (x, y, z) = self.player_anchor();
        api.spr_draw(SPR_PLAYER, x, y, z);
    }

    fn render_ghost(&self, gi: usize, api: &mut dyn CartApi) {
        let g = &self.ghosts[gi];
        let sprite_id = match g.mode {
            GhostMode::Normal => SPR_GHOST_BASE + gi as u8,
            GhostMode::Frightened => SPR_FRIGHT,
            GhostMode::Eaten => SPR_EATEN,
        };
        let (x, y, z) = self.ghost_anchor(gi);
        api.spr_draw(sprite_id, x, y, z);
    }

    /// Returns true if the tile in `dir` from `(tx, tz)` is traversable.
    fn can_move_to(&self, tx: i32, tz: i32, dir: (i32, i32)) -> bool {
        let nx = tx + dir.0;
        let nz = tz + dir.1;
        if !in_bounds(nx, nz) {
            return false;
        }
        self.maze[nx as usize][nz as usize] != Tile::Wall
    }

    /// Try to start a new player animation in `dir`. Returns true if started.
    fn start_player_move(&mut self, dir: (i32, i32)) -> bool {
        if dir == (0, 0) {
            return false;
        }
        if !self.can_move_to(self.player_tx, self.player_tz, dir) {
            return false;
        }
        self.player_anim = Some(Anim {
            src_tx: self.player_tx,
            src_tz: self.player_tz,
            dir,
            sub_step: 0,
            sub_frame: 0,
        });
        self.player_dir = dir;
        true
    }

    /// Run after the player's animation reaches sub_step == SUB_STEPS. Advances
    /// the logical tile, eats pellets, checks collision, and decides what's next.
    fn finalize_player_arrival(&mut self, t: u64, api: &mut dyn CartApi) {
        let anim = self.player_anim.expect("finalize called without active anim");
        let new_tx = anim.src_tx + anim.dir.0;
        let new_tz = anim.src_tz + anim.dir.1;
        self.player_tx = new_tx;
        self.player_tz = new_tz;
        self.player_anim = None;

        match self.maze[new_tx as usize][new_tz as usize] {
            Tile::Pellet => {
                self.maze[new_tx as usize][new_tz as usize] = Tile::Empty;
                self.pellets_remaining -= 1;
                self.score += 10;
            }
            Tile::PowerPellet => {
                self.maze[new_tx as usize][new_tz as usize] = Tile::Empty;
                self.pellets_remaining -= 1;
                self.score += 50;
                self.trigger_frightened(api);
            }
            _ => {}
        }

        self.check_end(t, api);
        if self.state != GameState::Playing {
            return;
        }

        // Classic controls: at each tile boundary, prefer the most recently
        // requested direction; if it's blocked, keep going in player_dir;
        // if that's blocked too, stop.
        let desired = self.desired_dir;
        if desired != (0, 0) && desired != self.player_dir
            && self.can_move_to(self.player_tx, self.player_tz, desired)
        {
            self.start_player_move(desired);
        } else if self.player_dir != (0, 0)
            && self.can_move_to(self.player_tx, self.player_tz, self.player_dir)
        {
            let dir = self.player_dir;
            self.start_player_move(dir);
        }
        // else: stop. player_anim stays None.
    }

    /// Advance the player's animation by one frame.
    fn tick_player_anim(&mut self, t: u64, api: &mut dyn CartApi) {
        let Some(mut anim) = self.player_anim else { return };
        let fpss = self.frames_per_sub_step_player();
        anim.sub_frame += 1;
        if anim.sub_frame < fpss {
            // Still inside this sub-step — no visual update needed.
            self.player_anim = Some(anim);
            return;
        }
        // Sub-step boundary reached. Tear down the previous render and either
        // render at the next sub-step position or finalize the move.
        let prev = Self::anim_anchor(&anim);
        Self::clear_entity_box(prev.0, prev.1, prev.2, api);
        // Stamp source tile (now vacated) and the imminent destination tile so
        // their static contents (pellets, walls) re-render under the entity.
        self.stamp_tile(anim.src_tx, anim.src_tz, api);
        let dst_tx = anim.src_tx + anim.dir.0;
        let dst_tz = anim.src_tz + anim.dir.1;
        if in_bounds(dst_tx, dst_tz) {
            self.stamp_tile(dst_tx, dst_tz, api);
        }

        anim.sub_frame = 0;
        anim.sub_step += 1;

        if anim.sub_step >= SUB_STEPS {
            // Arrived at the destination tile — write back, eat pellets, etc.
            self.player_anim = Some(anim);
            self.finalize_player_arrival(t, api);
            // Whether or not a fresh anim was started, the player needs to be
            // drawn on top of any underlying tile content.
            self.render_player(api);
            return;
        }

        self.player_anim = Some(anim);
        self.render_player(api);
    }

    /// Flip every Normal-mode ghost into Frightened, restart the chain counter,
    /// and pause the scatter/chase clock for FRIGHT_DURATION frames.
    fn trigger_frightened(&mut self, api: &mut dyn CartApi) {
        self.frightened_timer = FRIGHT_DURATION;
        self.ghost_chain = 0;
        for gi in 0..self.ghosts.len() {
            if self.ghosts[gi].mode == GhostMode::Normal {
                self.ghosts[gi].mode = GhostMode::Frightened;
                // Reverse direction (classic). If mid-anim, also flip the anim.
                let (dx, dz) = self.ghosts[gi].last_dir;
                self.ghosts[gi].last_dir = (-dx, -dz);
                self.ghosts[gi].anim = None;
                let (gx, gy, gz) = self.ghost_anchor(gi);
                Self::clear_entity_box(gx, gy, gz, api);
                self.stamp_tile(self.ghosts[gi].tx, self.ghosts[gi].tz, api);
                self.render_ghost(gi, api);
            }
        }
    }

    /// Compute the target tile for ghost `gi` given the current phase, player
    /// position, and player heading. Phase C will branch on Frightened/Eaten.
    fn target_for_ghost(&self, gi: usize) -> (i32, i32) {
        let g = &self.ghosts[gi];
        if self.phase == GamePhase::Scatter {
            return g.home_corner;
        }
        match g.behavior {
            GhostBehavior::Blinky => (self.player_tx, self.player_tz),
            GhostBehavior::Pinky => (
                self.player_tx + self.player_dir.0 * 4,
                self.player_tz + self.player_dir.1 * 4,
            ),
            GhostBehavior::Inky => {
                let blinky = self
                    .ghosts
                    .iter()
                    .find(|g| g.behavior == GhostBehavior::Blinky)
                    .map(|b| (b.tx, b.tz))
                    .unwrap_or((0, 0));
                let pivot_x = self.player_tx + self.player_dir.0 * 2;
                let pivot_z = self.player_tz + self.player_dir.1 * 2;
                (2 * pivot_x - blinky.0, 2 * pivot_z - blinky.1)
            }
            GhostBehavior::Clyde => {
                let dx = g.tx - self.player_tx;
                let dz = g.tz - self.player_tz;
                if dx.abs() + dz.abs() > 8 {
                    (self.player_tx, self.player_tz)
                } else {
                    g.home_corner
                }
            }
        }
    }

    /// Pick the neighbor tile that minimizes squared distance to `target`,
    /// using canonical up>left>down>right tiebreak. First pass forbids the
    /// reverse direction; if no valid choice, second pass allows it (cornered).
    fn pick_best_toward(&self, gi: usize, target: (i32, i32)) -> Option<(i32, i32)> {
        let g_tx = self.ghosts[gi].tx;
        let g_tz = self.ghosts[gi].tz;
        let last_dir = self.ghosts[gi].last_dir;
        let avoid = (-last_dir.0, -last_dir.1);
        let dirs = [(0, -1), (-1, 0), (0, 1), (1, 0)];

        let try_pick = |allow_reverse: bool| -> Option<(i32, i32)> {
            let mut best: Option<(i32, (i32, i32))> = None;
            for &(dx, dz) in &dirs {
                let nx = g_tx + dx;
                let nz = g_tz + dz;
                if !in_bounds(nx, nz) {
                    continue;
                }
                if self.maze[nx as usize][nz as usize] == Tile::Wall {
                    continue;
                }
                if !allow_reverse && (dx, dz) == avoid {
                    continue;
                }
                let dx_t = nx - target.0;
                let dz_t = nz - target.1;
                let dist = dx_t * dx_t + dz_t * dz_t;
                if best.map_or(true, |(d, _)| dist < d) {
                    best = Some((dist, (dx, dz)));
                }
            }
            best.map(|(_, d)| d)
        };

        try_pick(false).or_else(|| try_pick(true))
    }

    /// Frightened movement: pick a uniformly random valid neighbor.
    fn pick_random_neighbor(&mut self, gi: usize) -> Option<(i32, i32)> {
        let g_tx = self.ghosts[gi].tx;
        let g_tz = self.ghosts[gi].tz;
        let last_dir = self.ghosts[gi].last_dir;
        let avoid = (-last_dir.0, -last_dir.1);
        let dirs = [(0, -1), (-1, 0), (0, 1), (1, 0)];

        let mut valid = [(0i32, 0i32); 4];
        let mut count = 0usize;
        for &(dx, dz) in &dirs {
            let nx = g_tx + dx;
            let nz = g_tz + dz;
            if !in_bounds(nx, nz) {
                continue;
            }
            if self.maze[nx as usize][nz as usize] == Tile::Wall {
                continue;
            }
            if (dx, dz) == avoid {
                continue;
            }
            valid[count] = (dx, dz);
            count += 1;
        }
        if count == 0 {
            // Cornered — allow reverse.
            for &(dx, dz) in &dirs {
                let nx = g_tx + dx;
                let nz = g_tz + dz;
                if !in_bounds(nx, nz) {
                    continue;
                }
                if self.maze[nx as usize][nz as usize] == Tile::Wall {
                    continue;
                }
                valid[count] = (dx, dz);
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        let idx = (self.rng_step() as usize) % count;
        Some(valid[idx])
    }

    /// AI: pick which direction the ghost should head from its current tile.
    fn ghost_pick_dir(&mut self, gi: usize) -> Option<(i32, i32)> {
        let mode = self.ghosts[gi].mode;
        match mode {
            GhostMode::Normal => self.pick_best_toward(gi, self.target_for_ghost(gi)),
            GhostMode::Eaten => {
                let spawn = self.ghosts[gi].spawn;
                self.pick_best_toward(gi, spawn)
            }
            GhostMode::Frightened => self.pick_random_neighbor(gi),
        }
    }

    /// Start a fresh ghost animation in `dir` from the ghost's current tile.
    fn start_ghost_move(&mut self, gi: usize, dir: (i32, i32)) -> bool {
        let g = &mut self.ghosts[gi];
        if !in_bounds(g.tx + dir.0, g.tz + dir.1) {
            return false;
        }
        if self.maze[(g.tx + dir.0) as usize][(g.tz + dir.1) as usize] == Tile::Wall {
            return false;
        }
        g.anim = Some(Anim {
            src_tx: g.tx,
            src_tz: g.tz,
            dir,
            sub_step: 0,
            sub_frame: 0,
        });
        g.last_dir = dir;
        true
    }

    fn finalize_ghost_arrival(&mut self, gi: usize, t: u64, api: &mut dyn CartApi) {
        let anim = self
            .ghosts[gi]
            .anim
            .expect("finalize called without active ghost anim");
        let new_tx = anim.src_tx + anim.dir.0;
        let new_tz = anim.src_tz + anim.dir.1;
        let g = &mut self.ghosts[gi];
        g.tx = new_tx;
        g.tz = new_tz;
        g.anim = None;

        // Eaten → reached spawn? Flip back to Normal in place. Don't pick a new
        // direction this tick; the next tick's AI will start movement again.
        if g.mode == GhostMode::Eaten && (g.tx, g.tz) == g.spawn {
            g.mode = GhostMode::Normal;
            return;
        }

        self.check_end(t, api);
        if self.state != GameState::Playing {
            return;
        }

        // Pick the next direction immediately so the ghost is constantly in
        // motion (no idle frames between tile arrivals).
        if let Some(next_dir) = self.ghost_pick_dir(gi) {
            self.start_ghost_move(gi, next_dir);
        }
    }

    fn tick_ghost_anim(&mut self, gi: usize, t: u64, api: &mut dyn CartApi) {
        let Some(mut anim) = self.ghosts[gi].anim else { return };
        let fpss = Self::frames_per_sub_step_ghost(self.ghosts[gi].mode);
        anim.sub_frame += 1;
        if anim.sub_frame < fpss {
            self.ghosts[gi].anim = Some(anim);
            return;
        }

        let prev = Self::anim_anchor(&anim);
        Self::clear_entity_box(prev.0, prev.1, prev.2, api);
        self.stamp_tile(anim.src_tx, anim.src_tz, api);
        let dst_tx = anim.src_tx + anim.dir.0;
        let dst_tz = anim.src_tz + anim.dir.1;
        if in_bounds(dst_tx, dst_tz) {
            self.stamp_tile(dst_tx, dst_tz, api);
        }

        anim.sub_frame = 0;
        anim.sub_step += 1;

        if anim.sub_step >= SUB_STEPS {
            self.ghosts[gi].anim = Some(anim);
            self.finalize_ghost_arrival(gi, t, api);
            self.render_ghost(gi, api);
            return;
        }

        self.ghosts[gi].anim = Some(anim);
        self.render_ghost(gi, api);
    }

    fn advance_phase(&mut self) {
        let next = (self.phase_index + 1).min(PHASE_SCHEDULE.len() - 1);
        self.phase_index = next;
        let (phase, duration) = PHASE_SCHEDULE[next];
        self.phase = phase;
        self.phase_timer = duration;
        // Classic: ghosts reverse direction on phase change so the new target
        // pulls them away from where they were headed.
        for g in &mut self.ghosts {
            g.last_dir = (-g.last_dir.0, -g.last_dir.1);
        }
    }

    fn rng_step(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn check_end(&mut self, t: u64, api: &mut dyn CartApi) {
        let mut lost_life = false;
        for gi in 0..self.ghosts.len() {
            let g = &self.ghosts[gi];
            if g.tx == self.player_tx && g.tz == self.player_tz {
                match g.mode {
                    GhostMode::Normal => {
                        self.lives = self.lives.saturating_sub(1);
                        if self.lives == 0 {
                            self.state = GameState::Lost;
                            return;
                        }
                        lost_life = true;
                        break;
                    }
                    GhostMode::Frightened => {
                        // Eat the ghost. Score chain doubles per consecutive eat.
                        self.ghost_chain = (self.ghost_chain + 1).min(4);
                        let award = 200u32 << ((self.ghost_chain - 1).min(3) as u32);
                        self.score += award;
                        self.ghosts[gi].mode = GhostMode::Eaten;
                        // Don't re-render — the player overlay covers this cell;
                        // ghost re-renders as Eaten (white) on its next move.
                    }
                    GhostMode::Eaten => {
                        // Eyes-only ghost is harmless until it respawns.
                    }
                }
            }
        }
        if lost_life {
            self.soft_reset(t, api);
            return;
        }
        if self.pellets_remaining == 0 {
            self.state = GameState::Won;
        }
    }

    fn flood_end_state(&mut self, api: &mut dyn CartApi) {
        // Recolor all non-wall tiles to a single fill color. Walls keep their
        // structure; floor blocks become the flood color via vox_fill so the
        // whole 4³ block recolors uniformly.
        let fill = match self.state {
            GameState::Won => COLOR_PELLET, // pellet-yellow celebration
            GameState::Lost => 13,          // red
            GameState::Playing => return,
        };
        for tx in 0..MAZE_SIZE {
            for tz in 0..MAZE_SIZE {
                if self.maze[tx as usize][tz as usize] == Tile::Wall {
                    continue;
                }
                let (x, z) = Self::tile_to_world(tx, tz);
                api.vox_fill(x, GAME_Y, z, x + 3, GAME_Y + 3, z + 3, fill);
            }
        }
        // Always keep player visible on top.
        self.render_player(api);
    }
}

fn in_bounds(tx: i32, tz: i32) -> bool {
    tx >= 0 && tx < MAZE_SIZE && tz >= 0 && tz < MAZE_SIZE
}

fn generate_maze() -> Vec<Vec<Tile>> {
    let n = MAZE_SIZE as usize;
    let mut m = vec![vec![Tile::Pellet; n]; n];
    // Outer wall
    for i in 0..n {
        m[i][0] = Tile::Wall;
        m[i][n - 1] = Tile::Wall;
        m[0][i] = Tile::Wall;
        m[n - 1][i] = Tile::Wall;
    }
    // Internal symmetric wall blocks (placed in the upper-left quadrant, mirrored).
    let blocks: [(usize, usize, usize, usize); 3] = [
        (3, 3, 2, 1),
        (3, 6, 1, 2),
        (6, 3, 1, 1),
    ];
    for (bx, bz, w, h) in blocks {
        for dx in 0..w {
            for dz in 0..h {
                let tx = bx + dx;
                let tz = bz + dz;
                m[tx][tz] = Tile::Wall;
                m[n - 1 - tx][tz] = Tile::Wall;
                m[tx][n - 1 - tz] = Tile::Wall;
                m[n - 1 - tx][n - 1 - tz] = Tile::Wall;
            }
        }
    }
    // Central spawn area (3×3) with no pellets — Empty corridor.
    let c = (n / 2) as i32;
    for dx in -1..=1 {
        for dz in -1..=1 {
            m[(c + dx) as usize][(c + dz) as usize] = Tile::Empty;
        }
    }
    // 4 power pellets near the corners (interior, not on the outer wall).
    let power_positions = [(2, 2), (n - 3, 2), (2, n - 3), (n - 3, n - 3)];
    for (px, pz) in power_positions {
        if m[px][pz] != Tile::Wall {
            m[px][pz] = Tile::PowerPellet;
        }
    }
    m
}

impl PacmanCart {
    /// One-time sprite registration. Called from `init`. Sprites persist in the
    /// emulator across cart-driven restarts so we don't need to re-load.
    fn load_sprites(api: &mut dyn CartApi) {
        let _ = api.spr_load(SPR_WALL,  4, &make_solid_sprite_4(COLOR_WALL));
        let _ = api.spr_load(SPR_POWER, 2, &make_solid_sprite_2(COLOR_POWER));
        let _ = api.spr_load(SPR_PLAYER, 4, &make_solid_sprite_4(COLOR_PLAYER));
        for (i, &c) in COLOR_GHOST.iter().enumerate() {
            let _ = api.spr_load(SPR_GHOST_BASE + i as u8, 4, &make_solid_sprite_4(c));
        }
        let _ = api.spr_load(SPR_FRIGHT, 4, &make_solid_sprite_4(COLOR_FRIGHT));
        let _ = api.spr_load(SPR_EATEN,  4, &make_solid_sprite_4(COLOR_EATEN));
    }

    /// Reset all gameplay state and re-render the world. Used both on first
    /// init and on restart-after-game-over.
    fn setup_world(&mut self, api: &mut dyn CartApi) {
        self.score = 0;
        self.lives = LIVES_START;
        self.player_dir = (0, 0);
        self.desired_dir = (0, 0);
        self.player_anim = None;
        self.state = GameState::Playing;
        self.phase_index = 0;
        let (phase, duration) = PHASE_SCHEDULE[0];
        self.phase = phase;
        self.phase_timer = duration;
        self.frightened_timer = 0;
        self.ghost_chain = 0;
        self.respawn_until = 0;
        self.last_drawn_score = None;
        self.last_drawn_lives = None;
        self.announced_end = false;

        self.maze = generate_maze();
        let n = MAZE_SIZE;

        self.pellets_remaining = 0;
        for tx in 0..n {
            for tz in 0..n {
                match self.maze[tx as usize][tz as usize] {
                    Tile::Pellet | Tile::PowerPellet => self.pellets_remaining += 1,
                    _ => {}
                }
            }
        }

        // Player at center.
        self.player_tx = n / 2;
        self.player_tz = n / 2;

        // 4 ghosts, one per behavior, each at its assigned corner. The corners
        // double as `home_corner` (scatter target) and `spawn` (return-to when
        // eaten in phase C).
        let corners = [
            (n - 2, 1),       // Blinky — top-right
            (1, 1),           // Pinky — top-left
            (n - 2, n - 2),   // Inky — bottom-right
            (1, n - 2),       // Clyde — bottom-left
        ];
        let behaviors = [
            GhostBehavior::Blinky,
            GhostBehavior::Pinky,
            GhostBehavior::Inky,
            GhostBehavior::Clyde,
        ];
        self.ghosts = behaviors
            .iter()
            .zip(corners.iter())
            .enumerate()
            .map(|(i, (&behavior, &corner))| Ghost {
                tx: corner.0,
                tz: corner.1,
                color: COLOR_GHOST[i],
                last_dir: (0, 0),
                behavior,
                mode: GhostMode::Normal,
                home_corner: corner,
                spawn: corner,
                anim: None,
            })
            .collect();

        self.render_static_world(api);
        self.render_player(api);
        for gi in 0..self.ghosts.len() {
            self.render_ghost(gi, api);
        }

        // Title text floating above the maze.
        Self::draw_title(api, "Pacman v0.1!", COLOR_PELLET);

        // Initial HUD stamp.
        self.redraw_hud_if_dirty(api);

        api.print(&format!("Pellets: {}", self.pellets_remaining));
    }

    /// Wipe the display buffer and re-run setup_world. Called when the player
    /// presses Z on the game-over screen.
    fn restart_game(&mut self, api: &mut dyn CartApi) {
        api.vox_clear();
        self.setup_world(api);
    }

    /// Stamp the "PRESS Z" hint just under the end-state title, on the same
    /// XZFloor plane.
    fn draw_restart_prompt(api: &mut dyn CartApi) {
        let advance = api.text_advance() as i32;
        let height = api.text_height() as i32;
        let s = "PRESS Z";
        let max_w = (s.len() as i32) * advance;
        let z = TITLE_Z + 12;
        api.vox_fill(
            TITLE_X, TITLE_Y, z - height,
            TITLE_X + max_w, TITLE_Y + 1, z + 1,
            0,
        );
        api.text_draw_axis(s, TITLE_X, TITLE_Y, z, COLOR_PELLET, TextOrient::XZFloor);
    }
}

impl Cart for PacmanCart {
    fn init(&mut self, api: &mut dyn CartApi) {
        api.print("--- omnivixion: pacman v0 ---");
        api.print("WASD to move. Eat all pellets. Avoid the ghosts.");
        api.cam_pitch(75.0);
        Self::load_sprites(api);
        self.setup_world(api);
    }

    fn update(&mut self, api: &mut dyn CartApi, _dt: f32) {
        if self.state != GameState::Playing {
            if !self.announced_end {
                self.announced_end = true;
                let final_score = self.score;
                match self.state {
                    GameState::Won => {
                        api.print(&format!("YOU WIN! Final score: {}", final_score));
                        Self::draw_title(api, "WIN!", COLOR_PELLET);
                    }
                    GameState::Lost => {
                        api.print(&format!("Caught! Final score: {}", final_score));
                        Self::draw_title(api, "LOST", 13);
                    }
                    GameState::Playing => {}
                }
                self.flood_end_state(api);
                Self::draw_restart_prompt(api);
                // The HUD may still show pre-collision values (e.g. LIVES 1
                // when state flipped to Lost on the same tick); flush it.
                self.redraw_hud_if_dirty(api);
            }
            // Wait for the restart input — Z (action A) wipes everything and
            // calls setup_world for a fresh run.
            if api.btnp(6) {
                self.restart_game(api);
            }
            return;
        }

        let t = api.time();

        // Input → sticky desired_dir. The held direction is what the player
        // wants; tile-boundary logic decides if and when to actually turn.
        // btn 0=left, 1=right, 2=down (+z), 3=up (-z).
        if api.btn(3) {
            self.desired_dir = (0, -1);
        } else if api.btn(2) {
            self.desired_dir = (0, 1);
        } else if api.btn(0) {
            self.desired_dir = (-1, 0);
        } else if api.btn(1) {
            self.desired_dir = (1, 0);
        }

        // If the player is stopped at a tile, see if the held direction (or
        // current heading) can start a fresh move.
        if self.player_anim.is_none() {
            let desired = self.desired_dir;
            let current = self.player_dir;
            if desired != (0, 0)
                && self.can_move_to(self.player_tx, self.player_tz, desired)
            {
                self.start_player_move(desired);
            } else if current != (0, 0)
                && self.can_move_to(self.player_tx, self.player_tz, current)
            {
                self.start_player_move(current);
            }
        }

        // Advance the player's animation by one frame.
        self.tick_player_anim(t, api);
        if self.state != GameState::Playing {
            return;
        }

        // Ghosts hold still during the intro grace period and after each respawn;
        // once unlocked, each ghost runs its own animation independently.
        if t >= INTRO_GRACE && t >= self.respawn_until {
            for gi in 0..self.ghosts.len() {
                if self.ghosts[gi].anim.is_none() {
                    if let Some(dir) = self.ghost_pick_dir(gi) {
                        self.start_ghost_move(gi, dir);
                    }
                }
                self.tick_ghost_anim(gi, t, api);
                if self.state != GameState::Playing {
                    return;
                }
            }
        }

        // Frightened countdown — when it hits zero, all Frightened ghosts revert
        // to Normal in place and the phase timer resumes.
        if self.frightened_timer > 0 {
            self.frightened_timer -= 1;
            if self.frightened_timer == 0 {
                for gi in 0..self.ghosts.len() {
                    if self.ghosts[gi].mode == GhostMode::Frightened {
                        self.ghosts[gi].mode = GhostMode::Normal;
                        let (gx, gy, gz) = self.ghost_anchor(gi);
                        Self::clear_entity_box(gx, gy, gz, api);
                        self.render_ghost(gi, api);
                    }
                }
                self.ghost_chain = 0;
            }
        }

        // Advance the scatter/chase phase clock — paused while Frightened is active.
        if self.frightened_timer == 0 && self.phase_timer != u64::MAX {
            self.phase_timer = self.phase_timer.saturating_sub(1);
            if self.phase_timer == 0 {
                self.advance_phase();
            }
        }

        // Repaint the HUD when score / lives change.
        self.redraw_hud_if_dirty(api);
    }
}
