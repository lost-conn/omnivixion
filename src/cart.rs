//! Cart trait + the in-tree carts (DemoCart for the showcase scene, PacmanCart for the game).

use crate::console::CartApi;

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
//  PacmanCart — voxel pacman.
// ============================================================================

const MAZE_SIZE: i32 = 16;
const STRIDE: i32 = 2;
const MAZE_X0: i32 = 48;
const MAZE_Z0: i32 = 48;
const GAME_Y: i32 = 2;

const PLAYER_PERIOD: u64 = 8;   // frames between player moves while a key is held
const GHOST_PERIOD:  u64 = 18;  // frames between ghost moves
const INTRO_GRACE:   u64 = 60;  // ghosts hold still for 1 second after game start

const COLOR_FLOOR:  u8 = 4;   // grass — empty corridor
const COLOR_WALL:   u8 = 8;   // stone
const COLOR_PELLET: u8 = 11;  // yellow
const COLOR_PLAYER: u8 = 12;  // orange
const COLOR_GHOST: [u8; 3] = [13, 14, 15]; // red, pink, purple

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tile {
    Wall,
    Pellet,
    Empty,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GameState {
    Playing,
    Won,
    Lost,
}

struct Ghost {
    tx: i32,
    tz: i32,
    color: u8,
    last_dir: (i32, i32),
}

pub struct PacmanCart {
    maze: Vec<Vec<Tile>>,
    player_tx: i32,
    player_tz: i32,
    ghosts: Vec<Ghost>,
    pellets_remaining: u32,
    score: u32,
    state: GameState,
    rng: u64,
    last_player_move: u64,
    last_ghost_move: u64,
    announced_end: bool,
}

impl PacmanCart {
    pub fn new() -> Self {
        Self {
            maze: Vec::new(),
            player_tx: 0,
            player_tz: 0,
            ghosts: Vec::new(),
            pellets_remaining: 0,
            score: 0,
            state: GameState::Playing,
            rng: 0xDEAD_BEEF_CAFE_BABE,
            last_player_move: 0,
            last_ghost_move: 0,
            announced_end: false,
        }
    }

    fn tile_to_world(tx: i32, tz: i32) -> (i32, i32) {
        (MAZE_X0 + STRIDE * tx, MAZE_Z0 + STRIDE * tz)
    }

    fn tile_color(&self, tx: i32, tz: i32) -> u8 {
        match self.maze[tx as usize][tz as usize] {
            Tile::Wall => COLOR_WALL,
            Tile::Pellet => COLOR_PELLET,
            Tile::Empty => COLOR_FLOOR,
        }
    }

    fn render_static_world(&self, api: &mut dyn CartApi) {
        for tx in 0..MAZE_SIZE {
            for tz in 0..MAZE_SIZE {
                let (x, z) = Self::tile_to_world(tx, tz);
                api.vox_set(x, GAME_Y, z, self.tile_color(tx, tz));
            }
        }
    }

    fn render_player(&self, api: &mut dyn CartApi) {
        let (x, z) = Self::tile_to_world(self.player_tx, self.player_tz);
        api.vox_set(x, GAME_Y, z, COLOR_PLAYER);
    }

    fn render_ghost(&self, gi: usize, api: &mut dyn CartApi) {
        let g = &self.ghosts[gi];
        let (x, z) = Self::tile_to_world(g.tx, g.tz);
        api.vox_set(x, GAME_Y, z, g.color);
    }

    fn try_move_player(&mut self, dx: i32, dz: i32, api: &mut dyn CartApi) -> bool {
        let new_tx = self.player_tx + dx;
        let new_tz = self.player_tz + dz;
        if !in_bounds(new_tx, new_tz) {
            return false;
        }
        let new_tile = self.maze[new_tx as usize][new_tz as usize];
        if new_tile == Tile::Wall {
            return false;
        }
        // Repaint old tile from maze[][].
        let (ox, oz) = Self::tile_to_world(self.player_tx, self.player_tz);
        api.vox_set(ox, GAME_Y, oz, self.tile_color(self.player_tx, self.player_tz));

        self.player_tx = new_tx;
        self.player_tz = new_tz;

        if new_tile == Tile::Pellet {
            self.maze[new_tx as usize][new_tz as usize] = Tile::Empty;
            self.pellets_remaining -= 1;
            self.score += 10;
        }

        // Re-render player on top. Repaint any ghost we may have stepped onto in the
        // same frame so it stays visible after our own paint clears it next tick.
        self.render_player(api);
        true
    }

    fn move_ghost(&mut self, gi: usize, api: &mut dyn CartApi) {
        let player = (self.player_tx, self.player_tz);
        let g_tx = self.ghosts[gi].tx;
        let g_tz = self.ghosts[gi].tz;
        let last_dir = self.ghosts[gi].last_dir;
        let avoid = (-last_dir.0, -last_dir.1);
        let dirs = [(1, 0), (-1, 0), (0, 1), (0, -1)];

        let mut best: Option<(i32, (i32, i32))> = None;
        for &(dx, dz) in &dirs {
            let nx = g_tx + dx;
            let nz = g_tz + dz;
            if !in_bounds(nx, nz) { continue; }
            if self.maze[nx as usize][nz as usize] == Tile::Wall { continue; }
            if (dx, dz) == avoid { continue; }
            let dist = (nx - player.0).abs() + (nz - player.1).abs();
            match best {
                None => best = Some((dist, (dx, dz))),
                Some((d, _)) if dist < d => best = Some((dist, (dx, dz))),
                Some((d, _)) if dist == d && (self.rng_step() & 1) == 0 => {
                    best = Some((dist, (dx, dz)));
                }
                _ => {}
            }
        }
        if best.is_none() {
            // Cornered: allow reverse.
            for &(dx, dz) in &dirs {
                let nx = g_tx + dx;
                let nz = g_tz + dz;
                if !in_bounds(nx, nz) { continue; }
                if self.maze[nx as usize][nz as usize] == Tile::Wall { continue; }
                let dist = (nx - player.0).abs() + (nz - player.1).abs();
                match best {
                    None => best = Some((dist, (dx, dz))),
                    Some((d, _)) if dist < d => best = Some((dist, (dx, dz))),
                    _ => {}
                }
            }
        }
        let Some((_, (dx, dz))) = best else { return };

        // Repaint old tile from underlying state.
        let (ox, oz) = Self::tile_to_world(g_tx, g_tz);
        api.vox_set(ox, GAME_Y, oz, self.tile_color(g_tx, g_tz));

        let g = &mut self.ghosts[gi];
        g.tx += dx;
        g.tz += dz;
        g.last_dir = (dx, dz);

        self.render_ghost(gi, api);
    }

    fn rng_step(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn check_end(&mut self) {
        for g in &self.ghosts {
            if g.tx == self.player_tx && g.tz == self.player_tz {
                self.state = GameState::Lost;
                return;
            }
        }
        if self.pellets_remaining == 0 {
            self.state = GameState::Won;
        }
    }

    fn flood_end_state(&mut self, api: &mut dyn CartApi) {
        // Recolor all tiles to match final state.
        let fill = match self.state {
            GameState::Won => COLOR_PELLET,        // pellet-yellow celebration
            GameState::Lost => 13,                 // red
            GameState::Playing => return,
        };
        for tx in 0..MAZE_SIZE {
            for tz in 0..MAZE_SIZE {
                if self.maze[tx as usize][tz as usize] == Tile::Wall { continue; }
                let (x, z) = Self::tile_to_world(tx, tz);
                api.vox_set(x, GAME_Y, z, fill);
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
    m
}

impl Cart for PacmanCart {
    fn init(&mut self, api: &mut dyn CartApi) {
        api.print("--- omnivixion: pacman v0 ---");
        api.print("WASD to move. Eat all pellets. Avoid the ghosts.");
        api.cam_pitch(75.0);

        self.maze = generate_maze();
        let n = MAZE_SIZE;

        self.pellets_remaining = 0;
        for tx in 0..n {
            for tz in 0..n {
                if self.maze[tx as usize][tz as usize] == Tile::Pellet {
                    self.pellets_remaining += 1;
                }
            }
        }

        // Player at center.
        self.player_tx = n / 2;
        self.player_tz = n / 2;

        // 3 ghosts at far corners so the player has space to plan.
        self.ghosts = vec![
            Ghost { tx: 1,         tz: 1,         color: COLOR_GHOST[0], last_dir: (0, 0) },
            Ghost { tx: n - 2,     tz: 1,         color: COLOR_GHOST[1], last_dir: (0, 0) },
            Ghost { tx: 1,         tz: n - 2,     color: COLOR_GHOST[2], last_dir: (0, 0) },
        ];

        self.render_static_world(api);
        self.render_player(api);
        for gi in 0..self.ghosts.len() {
            self.render_ghost(gi, api);
        }

        api.print(&format!("Pellets: {}", self.pellets_remaining));
    }

    fn update(&mut self, api: &mut dyn CartApi, _dt: f32) {
        if self.state != GameState::Playing {
            if !self.announced_end {
                self.announced_end = true;
                let final_score = self.score;
                match self.state {
                    GameState::Won => api.print(&format!(
                        "YOU WIN! Final score: {}",
                        final_score
                    )),
                    GameState::Lost => api.print(&format!(
                        "Caught! Final score: {}",
                        final_score
                    )),
                    GameState::Playing => {}
                }
                self.flood_end_state(api);
            }
            return;
        }

        let t = api.time();

        // Player move on cooldown while a direction key is held.
        if t.saturating_sub(self.last_player_move) >= PLAYER_PERIOD {
            // btn 0=left (-x), 1=right (+x), 2=down (+z), 3=up (-z).
            let dir = if api.btn(3) {
                Some((0, -1))
            } else if api.btn(2) {
                Some((0, 1))
            } else if api.btn(0) {
                Some((-1, 0))
            } else if api.btn(1) {
                Some((1, 0))
            } else {
                None
            };
            if let Some((dx, dz)) = dir {
                if self.try_move_player(dx, dz, api) {
                    self.last_player_move = t;
                    self.check_end();
                    if self.state != GameState::Playing { return; }
                }
            }
        }

        // Ghosts hold still during the intro grace period.
        if t >= INTRO_GRACE && t.saturating_sub(self.last_ghost_move) >= GHOST_PERIOD {
            for gi in 0..self.ghosts.len() {
                self.move_ghost(gi, api);
            }
            self.last_ghost_move = t;
            self.check_end();
        }
    }
}
