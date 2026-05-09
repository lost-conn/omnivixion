import type { Cart, CartAPI } from './emulator';

const N = 128;
const SEA_LEVEL = 16;

// Heightfield: rolling terrain + central mountain.
function heightAt(x: number, z: number): number {
  const cx = x - 64;
  const cz = z - 64;
  const dist = Math.sqrt(cx * cx + cz * cz);

  let h = 18;
  h += 11 * Math.sin(x * 0.062) * Math.cos(z * 0.054);
  h += 6 * Math.sin((x + z) * 0.13);
  h += 4 * Math.cos(x * 0.21 - z * 0.18);
  h += 2.5 * Math.sin(x * 0.41) * Math.cos(z * 0.39);

  // Central mountain
  h += Math.max(0, 38 - dist * 0.78);

  // Distant rolling rim
  h += 6 * Math.exp(-Math.pow((dist - 50) / 14, 2));

  return Math.max(2, Math.min(95, Math.floor(h)));
}

function colorForElevation(y: number, top: number): number {
  if (y <= SEA_LEVEL + 1) {
    if (y === top) return 3; // sand on the surface near sea
    return 7; // dirt under sand
  }
  if (y < 28) return 4;  // grass
  if (y < 34) return 5;  // olive
  if (y < 50) return 8;  // stone
  if (y < 65) return 9;  // dark stone
  return 10;             // snow
}

let beaconBaseY = 0;
let mountainTopX = 64;
let mountainTopZ = 64;

export const cart: Cart = {
  init(api: CartAPI) {
    api.print('Generating world...');

    // Terrain
    for (let x = 0; x < N; x++) {
      for (let z = 0; z < N; z++) {
        const top = heightAt(x, z);
        for (let y = 0; y <= top; y++) {
          if (((x + y + z) & 1) !== 0) continue;
          api.vox_set(x, y, z, colorForElevation(y, top));
        }
      }
    }

    // Water (fills below sea level wherever terrain is shorter)
    for (let x = 0; x < N; x++) {
      for (let z = 0; z < N; z++) {
        const top = heightAt(x, z);
        for (let y = top + 1; y <= SEA_LEVEL; y++) {
          if (((x + y + z) & 1) !== 0) continue;
          api.vox_set(x, y, z, 2); // blue
        }
      }
    }

    // Trees scattered on grass-elevation land.
    for (let i = 0; i < 900; i++) {
      const x = (api.rand() * N) | 0;
      const z = (api.rand() * N) | 0;
      const top = heightAt(x, z);
      if (top < SEA_LEVEL + 2 || top > 32) continue;

      // Trunk: 4 cells (parity may make some no-op; visually fine)
      for (let dy = 1; dy <= 4; dy++) {
        api.vox_set(x, top + dy, z, 7);
      }
      // Leafy canopy
      for (let dx = -2; dx <= 2; dx++) {
        for (let dz = -2; dz <= 2; dz++) {
          for (let dy = 3; dy <= 7; dy++) {
            const cy = dy - 5;
            const r2 = dx * dx + dz * dz + cy * cy;
            if (r2 > 5.5) continue;
            api.vox_set(x + dx, top + dy, z + dz, 6);
          }
        }
      }
    }

    // Central tower on the mountain peak.
    let peakY = 0;
    for (let dx = -4; dx <= 4; dx++) {
      for (let dz = -4; dz <= 4; dz++) {
        const h = heightAt(64 + dx, 64 + dz);
        if (h > peakY) {
          peakY = h;
          mountainTopX = 64 + dx;
          mountainTopZ = 64 + dz;
        }
      }
    }
    const towerHeight = 22;
    for (let dy = 1; dy <= towerHeight; dy++) {
      const y = peakY + dy;
      const radius = 2 + (dy < 4 ? 1 : 0);
      for (let dx = -radius; dx <= radius; dx++) {
        for (let dz = -radius; dz <= radius; dz++) {
          if (dx * dx + dz * dz > radius * radius) continue;
          // Hollow at the very top
          if (dy >= towerHeight - 4 && dx * dx + dz * dz < (radius - 1) * (radius - 1)) continue;
          const cx = mountainTopX + dx;
          const cz = mountainTopZ + dz;
          api.vox_set(cx, y, cz, dy < 5 ? 9 : 8);
        }
      }
    }

    // Beacon parity-anchored at peak.
    beaconBaseY = peakY + towerHeight + 2;
    api.vox_set(mountainTopX, beaconBaseY, mountainTopZ, 13);

    // A few floating "lanterns" over the water.
    for (let i = 0; i < 12; i++) {
      const ang = (i / 12) * Math.PI * 2;
      const r = 22 + (i % 3) * 5;
      const x = (64 + Math.cos(ang) * r) | 0;
      const z = (64 + Math.sin(ang) * r) | 0;
      const y = SEA_LEVEL + 8 + (i % 3) * 2;
      // Find a valid parity cell near (x, y, z)
      for (let dy = 0; dy <= 1; dy++) {
        if (((x + (y + dy) + z) & 1) === 0) {
          api.vox_set(x, y + dy, z, 11); // yellow
          break;
        }
      }
    }

    api.print('done.');
  },

  update(api, _dt) {
    const t = api.time();

    // Animate the beacon: red ↔ yellow every ~half second at 60fps.
    const lit = (((t / 30) | 0) & 1) === 0;
    api.vox_set(mountainTopX, beaconBaseY, mountainTopZ, lit ? 13 : 11);

    // Pitch nudges: btn 4/5 (Shift/Space) or arrow up/down would be nice;
    // for the demo, leave pitch under emulator-mouse control.
    if (api.btnp(6)) {
      api.cam_pitch(Math.min(90, api.cam_pitch_get() + 10));
    }
    if (api.btnp(7)) {
      api.cam_pitch(Math.max(0, api.cam_pitch_get() - 10));
    }
  },
};
