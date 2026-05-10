//! `.omni` cart format loader.
//!
//! Parses an `.omni` text-format cart, validates header + assets, builds a
//! sandboxed Lua state, and returns a `Box<dyn Cart>` that wires the cart's
//! Lua `init`/`update` to the existing `CartApi` syscall surface.
//!
//! Format reference: `CART_FORMAT.md` at repo root.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine as _;
use mlua::{Function, Lua, MultiValue, Value, Variadic};
use serde::Deserialize;

use crate::cart::Cart;
use crate::console::{CartApi, TextOrient};

const FORMAT_MAJOR: u32 = 0;
const MAX_CART_BYTES: usize = 4 * 1024 * 1024;

// ---------------------------------------------------------------------------
//  Public entry points.
// ---------------------------------------------------------------------------

pub fn load_cart_from_path(path: &Path) -> Result<Box<dyn Cart>> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("reading cart {}", path.display()))?;
    if source.len() > MAX_CART_BYTES {
        bail!(
            "cart {} is {} bytes; hard cap is {}",
            path.display(),
            source.len(),
            MAX_CART_BYTES
        );
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("cart");
    load_cart_from_str(&source, name)
}

pub fn load_cart_from_str(source: &str, source_name: &str) -> Result<Box<dyn Cart>> {
    let raw = parse_omni(source)
        .with_context(|| format!("parsing cart {}", source_name))?;

    let header = parse_header(&raw.header)
        .with_context(|| format!("parsing __header__ in {}", source_name))?;
    validate_header(&header)?;

    if header.runtime.lang == "wasm" {
        bail!("WASM runtime is reserved for v0.2");
    }
    if raw.wasm.is_some() && header.runtime.lang != "wasm" {
        bail!("__wasm__ section present but runtime.lang is {:?}", header.runtime.lang);
    }
    if raw.lua.is_none() && header.runtime.lang == "lua" {
        bail!("runtime.lang = \"lua\" but no __lua__ section was provided");
    }

    let palette = parse_palettes(&raw.palettes)?;
    let sprites = parse_sprites(&raw.sprites)?;
    let data = parse_data(&raw.data)?;

    let lua_code = raw.lua.unwrap_or_default();
    let lua = build_lua_state(&lua_code, source_name)?;

    Ok(Box::new(LuaCart {
        lua,
        header,
        palette,
        sprites,
        data,
    }))
}

// ---------------------------------------------------------------------------
//  Header schema (TOML).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CartHeader {
    meta: HeaderMeta,
    #[serde(default)]
    runtime: HeaderRuntime,
    #[serde(default)]
    display: HeaderDisplay,
    #[serde(default)]
    #[allow(dead_code)]
    palette: HeaderPalette,
    #[serde(default)]
    #[allow(dead_code)]
    audio: HeaderAudio,
}

#[derive(Debug, Deserialize)]
struct HeaderMeta {
    spec: String,
    name: String,
    author: String,
    #[serde(default)]
    desc: String,
    #[serde(default)]
    #[allow(dead_code)]
    license: String,
    #[serde(default)]
    #[allow(dead_code)]
    homepage: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct HeaderRuntime {
    lang: String,
    hz: u32,
}
impl Default for HeaderRuntime {
    fn default() -> Self {
        Self { lang: "lua".to_string(), hz: 30 }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct HeaderDisplay {
    default_pitch: f32,
    persist_buffer: bool,
}
impl Default for HeaderDisplay {
    fn default() -> Self {
        Self { default_pitch: 30.0, persist_buffer: false }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct HeaderPalette {
    #[allow(dead_code)]
    default: u32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct HeaderAudio {
    #[allow(dead_code)]
    banks: u32,
}

fn parse_header(body: &str) -> Result<CartHeader> {
    let h: CartHeader = toml::from_str(body)
        .context("invalid TOML in __header__")?;
    Ok(h)
}

fn validate_header(h: &CartHeader) -> Result<()> {
    let (major, _minor) = parse_spec_version(&h.meta.spec)
        .context("invalid meta.spec")?;
    if major != FORMAT_MAJOR {
        bail!(
            "cart targets spec major {}, loader supports major {}",
            major, FORMAT_MAJOR
        );
    }
    if h.meta.name.is_empty() || h.meta.name.len() > 32 {
        bail!("meta.name must be 1..=32 chars");
    }
    if h.meta.author.is_empty() || h.meta.author.len() > 32 {
        bail!("meta.author must be 1..=32 chars");
    }
    if h.meta.desc.len() > 120 {
        bail!("meta.desc must be ≤ 120 chars");
    }
    if !matches!(h.runtime.lang.as_str(), "lua" | "wasm") {
        bail!("runtime.lang must be \"lua\" or \"wasm\", got {:?}", h.runtime.lang);
    }
    if !matches!(h.runtime.hz, 30 | 60) {
        bail!("runtime.hz must be 30 or 60, got {}", h.runtime.hz);
    }
    if !(0.0..=90.0).contains(&h.display.default_pitch) {
        bail!("display.default_pitch must be in [0, 90], got {}", h.display.default_pitch);
    }
    Ok(())
}

fn parse_spec_version(s: &str) -> Result<(u32, u32)> {
    let (maj, min) = s
        .split_once('.')
        .ok_or_else(|| anyhow!("expected MAJOR.MINOR, got {:?}", s))?;
    let major: u32 = maj.parse().with_context(|| format!("bad major in {:?}", s))?;
    let minor: u32 = min.parse().with_context(|| format!("bad minor in {:?}", s))?;
    Ok((major, minor))
}

// ---------------------------------------------------------------------------
//  Top-level section walker.
// ---------------------------------------------------------------------------

struct RawCart {
    header: String,
    lua: Option<String>,
    wasm: Option<String>,
    palettes: Vec<String>,
    sprites: Vec<String>,
    data: Vec<(String, String)>,
}

fn parse_omni(source: &str) -> Result<RawCart> {
    let mut lines = source.lines().enumerate();

    // Skip leading blanks; first non-blank line must be the magic line.
    let (magic_no, magic_line) = loop {
        match lines.next() {
            Some((n, l)) if l.trim().is_empty() => continue,
            Some((n, l)) => break (n + 1, l),
            None => bail!("empty cart"),
        }
    };
    let magic_trim = magic_line.trim();
    let version_str = magic_trim
        .strip_prefix("omnivixion cart v")
        .ok_or_else(|| anyhow!(
            "line {}: missing magic; expected `omnivixion cart vMAJOR.MINOR`",
            magic_no
        ))?;
    let version_token = version_str.split_whitespace().next().unwrap_or(version_str);
    let (file_major, _) = parse_spec_version(version_token)
        .with_context(|| format!("line {}: bad magic version", magic_no))?;
    if file_major != FORMAT_MAJOR {
        bail!(
            "line {}: cart magic declares major {}, loader supports major {}",
            magic_no, file_major, FORMAT_MAJOR
        );
    }

    // Walk remaining lines, splitting at section markers.
    let mut current: Option<(String, Option<String>, Vec<String>)> = None;
    let mut sections: Vec<(String, Option<String>, String)> = Vec::new();

    for (n, raw_line) in lines {
        if let Some((name, arg)) = parse_section_marker(raw_line) {
            if let Some((nm, ar, body_lines)) = current.take() {
                sections.push((nm, ar, body_lines.join("\n")));
            }
            current = Some((name, arg, Vec::new()));
        } else if let Some((_, _, body_lines)) = current.as_mut() {
            body_lines.push(raw_line.to_string());
        } else {
            let t = raw_line.trim();
            if !t.is_empty() && !t.starts_with('#') {
                bail!(
                    "line {}: content outside any section (before first `__section__`)",
                    n + 1
                );
            }
        }
    }
    if let Some((nm, ar, body_lines)) = current.take() {
        sections.push((nm, ar, body_lines.join("\n")));
    }

    if sections.is_empty() {
        bail!("cart has no sections; __header__ is required");
    }
    if sections[0].0 != "header" {
        bail!(
            "__header__ must be the first section (got __{}__)",
            sections[0].0
        );
    }

    let mut header: Option<String> = None;
    let mut lua_section: Option<String> = None;
    let mut wasm_section: Option<String> = None;
    let mut palettes: Vec<String> = Vec::new();
    let mut sprites: Vec<String> = Vec::new();
    let mut data: Vec<(String, String)> = Vec::new();

    for (name, arg, body) in sections {
        match (name.as_str(), arg.as_deref()) {
            ("header", None) => {
                if header.is_some() {
                    bail!("duplicate __header__ section");
                }
                header = Some(body);
            }
            ("header", Some(_)) => bail!("__header__ takes no argument"),
            ("lua", None) => {
                if lua_section.is_some() {
                    bail!("duplicate __lua__ section");
                }
                lua_section = Some(body);
            }
            ("lua", Some(_)) => bail!("__lua__ takes no argument"),
            ("wasm", None) => {
                if wasm_section.is_some() {
                    bail!("duplicate __wasm__ section");
                }
                wasm_section = Some(body);
            }
            ("wasm", Some(_)) => bail!("__wasm__ takes no argument"),
            ("palette", None) => palettes.push(body),
            ("palette", Some(_bank)) => {
                eprintln!("[loader] warning: __palette NAME__ banks reserved for v0.2; ignoring");
            }
            ("sprites", None) => sprites.push(body),
            ("sprites", Some(_)) => bail!("__sprites__ takes no argument"),
            ("data", Some(arg)) => {
                if !is_valid_identifier(arg) {
                    bail!("invalid __data NAME__ argument {:?}", arg);
                }
                if data.iter().any(|(n, _)| n == arg) {
                    bail!("duplicate __data {}__ section", arg);
                }
                data.push((arg.to_string(), body));
            }
            ("data", None) => bail!("__data__ requires a name argument"),
            ("source", None) => { /* documentation-only; ignore */ }
            (other, _) => {
                eprintln!("[loader] warning: unknown section __{}__ (ignored)", other);
            }
        }
    }

    let header = header.ok_or_else(|| anyhow!("missing __header__ section"))?;

    Ok(RawCart {
        header,
        lua: lua_section,
        wasm: wasm_section,
        palettes,
        sprites,
        data,
    })
}

fn parse_section_marker(line: &str) -> Option<(String, Option<String>)> {
    if line.len() < 5 {
        return None;
    }
    if !line.starts_with("__") || !line.ends_with("__") {
        return None;
    }
    let inner = &line[2..line.len() - 2];
    if inner.is_empty() {
        return None;
    }
    let parts: Vec<&str> = inner.split_whitespace().collect();
    match parts.len() {
        1 => {
            if !is_valid_identifier(parts[0]) {
                return None;
            }
            Some((parts[0].to_string(), None))
        }
        2 => {
            if !is_valid_identifier(parts[0]) || !is_valid_identifier(parts[1]) {
                return None;
            }
            Some((parts[0].to_string(), Some(parts[1].to_string())))
        }
        _ => None,
    }
}

fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ---------------------------------------------------------------------------
//  __palette__ parser.
// ---------------------------------------------------------------------------

fn parse_palettes(bodies: &[String]) -> Result<Vec<(u8, u8, u8, u8)>> {
    let mut map: HashMap<u8, (u8, u8, u8)> = HashMap::new();
    for body in bodies {
        for (n, line) in body.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let (idx_str, rgb_str) = t
                .split_once(':')
                .ok_or_else(|| anyhow!(
                    "palette line {}: expected `INDEX: RRGGBB`, got {:?}",
                    n + 1, t
                ))?;
            let idx: u8 = idx_str.trim().parse()
                .map_err(|_| anyhow!("palette line {}: bad index {:?}", n + 1, idx_str.trim()))?;
            if idx > 15 {
                bail!("palette line {}: index {} out of range 0..=15", n + 1, idx);
            }
            if idx == 0 {
                continue;
            }
            // Body may have trailing comment after the hex.
            let hex = rgb_str
                .split('#')
                .next()
                .unwrap()
                .trim();
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                bail!(
                    "palette line {}: expected 6-digit hex RRGGBB, got {:?}",
                    n + 1, hex
                );
            }
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
            map.insert(idx, (r, g, b));
        }
    }
    let mut out: Vec<(u8, u8, u8, u8)> = map
        .into_iter()
        .map(|(k, (r, g, b))| (k, r, g, b))
        .collect();
    out.sort_by_key(|t| t.0);
    Ok(out)
}

// ---------------------------------------------------------------------------
//  __sprites__ parser.
// ---------------------------------------------------------------------------

struct SpriteAsset {
    id: u8,
    size: u8,
    bytes: Vec<u8>,
}

fn parse_sprites(bodies: &[String]) -> Result<Vec<SpriteAsset>> {
    let mut out = Vec::new();
    for body in bodies {
        // Pre-filter to (1-indexed line_no, trimmed) for non-blank, non-comment.
        let lines: Vec<(usize, &str)> = body
            .lines()
            .enumerate()
            .filter_map(|(n, l)| {
                let t = l.trim();
                if t.is_empty() || t.starts_with('#') {
                    None
                } else {
                    Some((n + 1, t))
                }
            })
            .collect();

        let mut i = 0;
        while i < lines.len() {
            let (header_no, header) = lines[i];
            let parts: Vec<&str> = header.split_whitespace().collect();
            if parts.first() != Some(&"sprite") {
                bail!(
                    "sprite section line {}: expected `sprite ID NAME SX SY SZ`, got {:?}",
                    header_no, header
                );
            }
            if parts.len() != 6 {
                bail!(
                    "sprite section line {}: expected 6 tokens, got {}",
                    header_no, parts.len()
                );
            }
            let id: u8 = parts[1].parse()
                .map_err(|_| anyhow!("line {}: bad sprite id {:?}", header_no, parts[1]))?;
            let name = parts[2];
            if !is_valid_identifier(name) {
                bail!(
                    "line {}: invalid sprite name {:?}",
                    header_no, name
                );
            }
            let sx: u8 = parts[3].parse()
                .map_err(|_| anyhow!("line {}: bad sx {:?}", header_no, parts[3]))?;
            let sy: u8 = parts[4].parse()
                .map_err(|_| anyhow!("line {}: bad sy {:?}", header_no, parts[4]))?;
            let sz: u8 = parts[5].parse()
                .map_err(|_| anyhow!("line {}: bad sz {:?}", header_no, parts[5]))?;
            if sx != sy || sy != sz {
                bail!(
                    "line {}: v0.1 supports cubic sprites only ({}x{}x{})",
                    header_no, sx, sy, sz
                );
            }
            let size = sx;
            if !matches!(size, 2 | 4 | 8 | 16) {
                bail!(
                    "line {}: sprite size must be 2, 4, 8, or 16 (got {})",
                    header_no, size
                );
            }
            i += 1;

            let mut slices: Vec<Option<Vec<Vec<char>>>> =
                (0..size as usize).map(|_| None).collect();

            while i < lines.len() {
                let (yl_no, yl) = lines[i];
                if yl.starts_with("sprite ") || yl == "sprite" {
                    break;
                }
                let n_str = yl.strip_prefix("y=").ok_or_else(|| anyhow!(
                    "line {}: expected `y=N` or `sprite ...`, got {:?}",
                    yl_no, yl
                ))?;
                let y: usize = n_str.parse()
                    .map_err(|_| anyhow!("line {}: bad y index {:?}", yl_no, n_str))?;
                if y >= size as usize {
                    bail!(
                        "line {}: y={} out of range (sprite size {})",
                        yl_no, y, size
                    );
                }
                if slices[y].is_some() {
                    bail!("line {}: duplicate y={} slice", yl_no, y);
                }
                i += 1;

                let mut grid: Vec<Vec<char>> = Vec::with_capacity(size as usize);
                for _ in 0..size as usize {
                    if i >= lines.len() {
                        bail!(
                            "sprite '{}' (line {}): y={} truncated, expected {} rows",
                            name, header_no, y, size
                        );
                    }
                    let (rl_no, rl) = lines[i];
                    if rl.starts_with("sprite ") || rl.starts_with("y=") {
                        bail!(
                            "line {}: y={} truncated (expected {} rows, got {})",
                            rl_no, y, size, grid.len()
                        );
                    }
                    let row: Vec<char> = rl
                        .split_whitespace()
                        .map(|tok| {
                            let mut chars = tok.chars();
                            let c = chars.next();
                            if chars.next().is_some() || c.is_none() {
                                None
                            } else {
                                Some(c.unwrap())
                            }
                        })
                        .collect::<Option<Vec<char>>>()
                        .ok_or_else(|| anyhow!(
                            "line {}: each glyph must be a single character",
                            rl_no
                        ))?;
                    if row.len() != size as usize {
                        bail!(
                            "line {}: expected {} glyphs, got {}",
                            rl_no, size, row.len()
                        );
                    }
                    grid.push(row);
                    i += 1;
                }
                slices[y] = Some(grid);
            }

            let bytes = pack_sprite(size, &slices)
                .map_err(|e| anyhow!("sprite '{}' (line {}): {}", name, header_no, e))?;
            out.push(SpriteAsset { id, size, bytes });
        }
    }
    Ok(out)
}

fn pack_sprite(size: u8, slices: &[Option<Vec<Vec<char>>>]) -> Result<Vec<u8>> {
    let n = size as usize;
    let half = n / 2;
    let bytes_len = n * n * half;
    let mut bytes = vec![0u8; bytes_len];

    for y in 0..n {
        let Some(grid) = &slices[y] else {
            // Unspecified slice: every parity-even cell stays 0 (transparent).
            // No need to validate parity-odd glyphs since we have no rows.
            continue;
        };
        for z in 0..n {
            for x in 0..n {
                let glyph = grid[z][x];
                let parity_even = (x + y + z) % 2 == 0;
                if !parity_even {
                    if glyph != '.' {
                        bail!(
                            "y={} z={} x={}: parity-odd cell must be `.`, got {:?}",
                            y, z, x, glyph
                        );
                    }
                    continue;
                }
                let color = match glyph {
                    '.' => bail!(
                        "y={} z={} x={}: parity-even cell must be 0..f, got `.`",
                        y, z, x
                    ),
                    '0'..='9' => glyph as u8 - b'0',
                    'a'..='f' => 10 + (glyph as u8 - b'a'),
                    _ => bail!(
                        "y={} z={} x={}: glyph must be `.`, 0..9, or a..f (got {:?})",
                        y, z, x, glyph
                    ),
                };
                let rel_idx = z * n * half + y * half + (x >> 1);
                if (rel_idx & 1) == 0 {
                    bytes[rel_idx >> 1] |= color & 0x0f;
                } else {
                    bytes[rel_idx >> 1] |= (color & 0x0f) << 4;
                }
            }
        }
    }
    Ok(bytes)
}

// ---------------------------------------------------------------------------
//  __data NAME__ parser.
// ---------------------------------------------------------------------------

fn parse_data(items: &[(String, String)]) -> Result<HashMap<String, Vec<u8>>> {
    use base64::engine::general_purpose::STANDARD;
    let mut map = HashMap::new();
    for (name, body) in items {
        let cleaned: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
            .collect();
        let bytes = STANDARD
            .decode(cleaned.as_bytes())
            .with_context(|| format!("decoding __data {}__", name))?;
        map.insert(name.clone(), bytes);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
//  Lua state setup.
// ---------------------------------------------------------------------------

const SANDBOX_REMOVE: &[&str] = &[
    "package",
    "require",
    "dofile",
    "loadfile",
    "load",
    "loadstring",
    "os",
    "io",
    "debug",
    "collectgarbage",
    "coroutine",
];

fn build_lua_state(code: &str, source_name: &str) -> Result<Lua> {
    let lua = Lua::new();
    install_sandbox(&lua).map_err(|e| anyhow!("installing sandbox: {}", e))?;
    lua.load(code)
        .set_name(source_name)
        .exec()
        .map_err(|e| anyhow!("compiling __lua__: {}", e))?;
    Ok(lua)
}

fn install_sandbox(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    for name in SANDBOX_REMOVE {
        g.set(*name, Value::Nil)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
//  LuaCart — Cart trait impl backed by an mlua Lua state.
// ---------------------------------------------------------------------------

struct LuaCart {
    lua: Lua,
    header: CartHeader,
    palette: Vec<(u8, u8, u8, u8)>,
    sprites: Vec<SpriteAsset>,
    data: HashMap<String, Vec<u8>>,
}

impl Cart for LuaCart {
    fn init(&mut self, api: &mut dyn CartApi) {
        // Header-driven setup, applied before user code so the cart sees the
        // requested palette / sprite bank / pitch / persist flag from frame 1.
        for &(slot, r, g, b) in &self.palette {
            api.pal_set(
                slot,
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
            );
        }
        for s in &self.sprites {
            api.spr_load(s.id, s.size, &s.bytes);
        }
        if self.header.display.persist_buffer {
            api.set_persist_buffer(true);
        }
        api.cam_pitch(self.header.display.default_pitch);

        let _ = self.run_lua_fn("init", api, |_lua| Ok(MultiValue::new()));
        // Lua errors abort the run; matches the renderer-init `expect` style.
    }

    fn update(&mut self, api: &mut dyn CartApi, dt: f32) {
        let _ = self.run_lua_fn("update", api, |_lua| {
            let mut mv = MultiValue::new();
            mv.push_back(Value::Number(dt as f64));
            Ok(mv)
        });
    }
}

impl LuaCart {
    fn run_lua_fn<F>(&self, fn_name: &'static str, api: &mut dyn CartApi, args_for: F)
    where
        F: FnOnce(&Lua) -> mlua::Result<MultiValue>,
    {
        // Skip if the cart didn't define this hook.
        let exists = matches!(
            self.lua.globals().get::<Value>(fn_name).ok(),
            Some(Value::Function(_))
        );
        if !exists {
            return;
        }

        let api_cell_owned: RefCell<&mut dyn CartApi> = RefCell::new(api);
        let result: mlua::Result<()> = self.lua.scope(|scope| {
            // Each scoped closure is `move`, capturing a Copy of the &RefCell so
            // multiple closures can share the cell without re-borrowing it.
            let api_cell: &RefCell<&mut dyn CartApi> = &api_cell_owned;
            let data: &HashMap<String, Vec<u8>> = &self.data;
            let lua = &self.lua;
            let g = lua.globals();

            // -- voxel ops ------------------------------------------------------
            g.set("vox_set", scope.create_function_mut(
                move |_, (x, y, z, c): (i32, i32, i32, u8)| {
                    api_cell.borrow_mut().vox_set(x, y, z, c);
                    Ok(())
                },
            )?)?;
            g.set("vox_get", scope.create_function_mut(
                move |_, (x, y, z): (i32, i32, i32)| {
                    Ok(api_cell.borrow_mut().vox_get(x, y, z))
                },
            )?)?;
            g.set("vox_clear", scope.create_function_mut(move |_, ()| {
                api_cell.borrow_mut().vox_clear();
                Ok(())
            })?)?;
            g.set("vox_fill", scope.create_function_mut(
                move |_, (x0, y0, z0, x1, y1, z1, c): (i32, i32, i32, i32, i32, i32, u8)| {
                    api_cell.borrow_mut().vox_fill(x0, y0, z0, x1, y1, z1, c);
                    Ok(())
                },
            )?)?;
            g.set("vox_is_valid", scope.create_function_mut(
                move |_, (x, y, z): (i32, i32, i32)| {
                    Ok(api_cell.borrow_mut().vox_is_valid(x, y, z))
                },
            )?)?;
            g.set("neighbor", scope.create_function_mut(
                move |_, (x, y, z, idx): (i32, i32, i32, u8)| {
                    let (nx, ny, nz) = api_cell.borrow_mut().neighbor(x, y, z, idx);
                    Ok((nx, ny, nz))
                },
            )?)?;

            // -- sprites --------------------------------------------------------
            g.set("spr_load", scope.create_function_mut(
                move |_, (id, size, data_str): (u8, u8, mlua::String)| {
                    let bytes = data_str.as_bytes();
                    Ok(api_cell.borrow_mut().spr_load(id, size, &bytes))
                },
            )?)?;
            g.set("spr_draw", scope.create_function_mut(
                move |_, (id, x, y, z): (u8, i32, i32, i32)| {
                    api_cell.borrow_mut().spr_draw(id, x, y, z);
                    Ok(())
                },
            )?)?;
            g.set("spr_clear", scope.create_function_mut(move |_, id: u8| {
                api_cell.borrow_mut().spr_clear(id);
                Ok(())
            })?)?;

            // -- buffer mode ----------------------------------------------------
            g.set("set_persist_buffer", scope.create_function_mut(
                move |_, persist: bool| {
                    api_cell.borrow_mut().set_persist_buffer(persist);
                    Ok(())
                },
            )?)?;
            g.set("persist_buffer", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().persist_buffer())
            })?)?;

            // -- text -----------------------------------------------------------
            g.set("text_draw", scope.create_function_mut(
                move |_, (s, x, y, z, color): (String, i32, i32, i32, u8)| {
                    api_cell.borrow_mut().text_draw(&s, x, y, z, color);
                    Ok(())
                },
            )?)?;
            g.set("text_draw_axis", scope.create_function_mut(
                move |_, (s, x, y, z, color, orient):
                    (String, i32, i32, i32, u8, String)| {
                    let o = match orient.as_str() {
                        "xy_wall" => TextOrient::XYWall,
                        "xz_floor" => TextOrient::XZFloor,
                        "zy_wall" => TextOrient::ZYWall,
                        other => return Err(mlua::Error::external(format!(
                            "unknown TextOrient {:?} (want xy_wall|xz_floor|zy_wall)",
                            other
                        ))),
                    };
                    api_cell.borrow_mut().text_draw_axis(&s, x, y, z, color, o);
                    Ok(())
                },
            )?)?;
            g.set("text_advance", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().text_advance())
            })?)?;
            g.set("text_height", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().text_height())
            })?)?;

            // -- palette --------------------------------------------------------
            g.set("pal_set", scope.create_function_mut(
                move |_, (slot, r, g_, b): (u8, f32, f32, f32)| {
                    api_cell.borrow_mut().pal_set(slot, r, g_, b);
                    Ok(())
                },
            )?)?;
            g.set("pal_reset", scope.create_function_mut(move |_, ()| {
                api_cell.borrow_mut().pal_reset();
                Ok(())
            })?)?;

            // -- camera ---------------------------------------------------------
            g.set("cam_pitch", scope.create_function_mut(move |_, deg: f32| {
                api_cell.borrow_mut().cam_pitch(deg);
                Ok(())
            })?)?;
            g.set("cam_pitch_get", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().cam_pitch_get())
            })?)?;

            // -- input + time + rng --------------------------------------------
            g.set("btn", scope.create_function_mut(move |_, idx: u8| {
                Ok(api_cell.borrow_mut().btn(idx))
            })?)?;
            g.set("btnp", scope.create_function_mut(move |_, idx: u8| {
                Ok(api_cell.borrow_mut().btnp(idx))
            })?)?;
            g.set("time", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().time())
            })?)?;
            g.set("rand", scope.create_function_mut(move |_, ()| {
                Ok(api_cell.borrow_mut().rand())
            })?)?;

            // -- print (rebound to api.print, variadic + tostring) -------------
            g.set("print", scope.create_function_mut(
                move |lua, args: Variadic<Value>| -> mlua::Result<()> {
                    let tostring: Function = lua.globals().get("tostring")?;
                    let mut buf = String::new();
                    for (i, v) in args.into_iter().enumerate() {
                        if i > 0 {
                            buf.push(' ');
                        }
                        let s: String = tostring.call(v)?;
                        buf.push_str(&s);
                    }
                    api_cell.borrow_mut().print(&buf);
                    Ok(())
                },
            )?)?;

            // -- data section access -------------------------------------------
            g.set("data_get", scope.create_function(
                move |lua, name: String| -> mlua::Result<Value> {
                    match data.get(&name) {
                        Some(bytes) => Ok(Value::String(lua.create_string(bytes)?)),
                        None => Ok(Value::Nil),
                    }
                },
            )?)?;

            // Call the user function.
            let f: Function = lua.globals().get(fn_name)?;
            let args = args_for(lua)?;
            f.call::<()>(args)?;
            Ok(())
        });

        if let Err(e) = result {
            panic!("[loader] cart Lua `{}` failed: {}", fn_name, e);
        }
    }
}
