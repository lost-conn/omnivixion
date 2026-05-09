//! Rhombic dodecahedral lattice (FCC).
//!
//! Cells live at integer (x, y, z) where (x + y + z) is even.
//! Each cell has 12 face-neighbors at offsets that are permutations of (±1, ±1, 0).

use glam::Vec3;

pub const N: i32 = 128;
const N_U: usize = N as usize;
const HALF: usize = N_U / 2;

/// Number of parity-valid cells in the 128³ lattice. `N³ / 2 = 1,048,576`.
pub const VALID_CELLS: usize = (N_U * N_U * N_U) / 2;

/// 12 face-neighbor offsets. Order is part of the spec (cart referencable by index).
pub const NEIGHBOR_OFFSETS: [[i32; 3]; 12] = [
    [ 1,  1,  0], [ 1, -1,  0], [-1,  1,  0], [-1, -1,  0],
    [ 1,  0,  1], [ 1,  0, -1], [-1,  0,  1], [-1,  0, -1],
    [ 0,  1,  1], [ 0,  1, -1], [ 0, -1,  1], [ 0, -1, -1],
];

#[inline]
pub fn is_valid(x: i32, y: i32, z: i32) -> bool {
    x >= 0
        && x < N
        && y >= 0
        && y < N
        && z >= 0
        && z < N
        && ((x + y + z) & 1) == 0
}

/// Compact cell index in `[0, VALID_CELLS)`.
///
/// Since `(x + y + z)` is even, x's parity is determined by `(y + z) & 1`,
/// so packing `x_packed = x >> 1` gives 64 valid x-positions per `(y, z)` slot
/// without losing information. Layout: `z * N * N/2 + y * N/2 + (x >> 1)`.
#[inline]
pub fn cell_index(x: i32, y: i32, z: i32) -> usize {
    (z as usize) * N_U * HALF + (y as usize) * HALF + ((x as usize) >> 1)
}

#[inline]
pub fn index_to_cell(idx: usize) -> (i32, i32, i32) {
    let z = idx / (N_U * HALF);
    let rem = idx - z * N_U * HALF;
    let y = rem / HALF;
    let x_packed = rem - y * HALF;
    let x = (x_packed << 1) | ((y + z) & 1);
    (x as i32, y as i32, z as i32)
}

/// Vertices of the rhombic face perpendicular to the given offset.
/// Returns 4 vertices wound CCW from outside.
fn rd_face_verts(offset: [i32; 3]) -> [Vec3; 4] {
    let (a, b, c) = (offset[0] as f32, offset[1] as f32, offset[2] as f32);
    let mut verts = if a == 0.0 {
        [
            Vec3::new(0.0, b, 0.0),
            Vec3::new(0.5, b * 0.5, c * 0.5),
            Vec3::new(0.0, 0.0, c),
            Vec3::new(-0.5, b * 0.5, c * 0.5),
        ]
    } else if b == 0.0 {
        [
            Vec3::new(a, 0.0, 0.0),
            Vec3::new(a * 0.5, 0.5, c * 0.5),
            Vec3::new(0.0, 0.0, c),
            Vec3::new(a * 0.5, -0.5, c * 0.5),
        ]
    } else {
        [
            Vec3::new(a, 0.0, 0.0),
            Vec3::new(a * 0.5, b * 0.5, 0.5),
            Vec3::new(0.0, b, 0.0),
            Vec3::new(a * 0.5, b * 0.5, -0.5),
        ]
    };
    let edge1 = verts[1] - verts[0];
    let edge2 = verts[2] - verts[0];
    let normal = edge1.cross(edge2);
    let outward = Vec3::new(a, b, c);
    if normal.dot(outward) < 0.0 {
        verts.swap(1, 3);
    }
    verts
}

/// Build the RD mesh as non-indexed triangles for flat shading.
/// Returns (positions, normals) per vertex; 24 triangles × 3 verts = 72 entries.
pub fn build_rd_mesh() -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut positions = Vec::with_capacity(72);
    let mut normals = Vec::with_capacity(72);

    for offset in NEIGHBOR_OFFSETS.iter() {
        let v = rd_face_verts(*offset);
        let n = Vec3::new(offset[0] as f32, offset[1] as f32, offset[2] as f32).normalize();
        // tri 1: v0, v1, v2
        for vi in [0, 1, 2] {
            positions.push([v[vi].x, v[vi].y, v[vi].z]);
            normals.push([n.x, n.y, n.z]);
        }
        // tri 2: v0, v2, v3
        for vi in [0, 2, 3] {
            positions.push([v[vi].x, v[vi].y, v[vi].z]);
            normals.push([n.x, n.y, n.z]);
        }
    }

    (positions, normals)
}
