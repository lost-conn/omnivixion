// Rhombic dodecahedral lattice (FCC).
// Spec: cells at integer (x,y,z) with (x+y+z) even.
// 12 face-neighbors at offsets that are permutations of (±1, ±1, 0).

export type Cell = { x: number; y: number; z: number };
export type Vec3 = [number, number, number];

export function isValid(c: Cell): boolean {
  return ((c.x + c.y + c.z) & 1) === 0;
}

export function key(c: Cell): string {
  return `${c.x},${c.y},${c.z}`;
}

export function fromKey(k: string): Cell {
  const [x, y, z] = k.split(',').map(Number);
  return { x, y, z };
}

export function cellToWorld(c: Cell): Vec3 {
  return [c.x, c.y, c.z];
}

export const NEIGHBOR_OFFSETS: readonly Vec3[] = [
  [ 1,  1,  0], [ 1, -1,  0], [-1,  1,  0], [-1, -1,  0], // XY group
  [ 1,  0,  1], [ 1,  0, -1], [-1,  0,  1], [-1,  0, -1], // XZ group
  [ 0,  1,  1], [ 0,  1, -1], [ 0, -1,  1], [ 0, -1, -1], // YZ group
];

export function neighbor(c: Cell, faceIndex: number): Cell {
  const [dx, dy, dz] = NEIGHBOR_OFFSETS[faceIndex];
  return { x: c.x + dx, y: c.y + dy, z: c.z + dz };
}

export function neighbors(c: Cell): Cell[] {
  return NEIGHBOR_OFFSETS.map(([dx, dy, dz]) => ({
    x: c.x + dx,
    y: c.y + dy,
    z: c.z + dz,
  }));
}

// Vertices of one rhombic face, ordered CCW viewed from outside.
// offset has exactly one zero component; other two are ±1.
function rdFaceVerts(offset: Vec3): Vec3[] {
  const [a, b, c] = offset;
  let verts: Vec3[];
  if (a === 0) {
    verts = [[0, b, 0], [ 0.5, b / 2, c / 2], [0, 0, c], [-0.5, b / 2, c / 2]];
  } else if (b === 0) {
    verts = [[a, 0, 0], [a / 2,  0.5, c / 2], [0, 0, c], [a / 2, -0.5, c / 2]];
  } else {
    verts = [[a, 0, 0], [a / 2, b / 2,  0.5], [0, b, 0], [a / 2, b / 2, -0.5]];
  }
  // Force CCW outward: cross of (v1-v0) and (v2-v0) must align with offset.
  const [v0, v1, v2] = verts;
  const ex = v1[0] - v0[0], ey = v1[1] - v0[1], ez = v1[2] - v0[2];
  const fx = v2[0] - v0[0], fy = v2[1] - v0[1], fz = v2[2] - v0[2];
  const nx = ey * fz - ez * fy;
  const ny = ez * fx - ex * fz;
  const nz = ex * fy - ey * fx;
  if (nx * a + ny * b + nz * c < 0) {
    verts = [verts[0], verts[3], verts[2], verts[1]];
  }
  return verts;
}

export type RdGeometry = {
  positions: Float32Array;   // length = 72*3 (24 tris * 3 verts * 3 coords), non-indexed
  faceIndexPerTriangle: Uint8Array; // length = 24, value 0..11
  triangleCount: number;
};

// Non-indexed mesh so flat shading + computeVertexNormals works without averaging.
export const buildGeometry = buildRdGeometry;
export function buildRdGeometry(): RdGeometry {
  const positions: number[] = [];
  const faceIdx: number[] = [];
  for (let f = 0; f < 12; f++) {
    const v = rdFaceVerts(NEIGHBOR_OFFSETS[f]);
    // quad → tri (0,1,2) + tri (0,2,3)
    positions.push(...v[0], ...v[1], ...v[2]);
    positions.push(...v[0], ...v[2], ...v[3]);
    faceIdx.push(f, f);
  }
  return {
    positions: new Float32Array(positions),
    faceIndexPerTriangle: new Uint8Array(faceIdx),
    triangleCount: faceIdx.length,
  };
}
