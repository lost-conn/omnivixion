import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { NEIGHBOR_OFFSETS, buildRdGeometry } from './rd';

export const N = 128;
const N2 = N * N;
const N3 = N * N * N;
const MAX_INSTANCES = 1_048_576;

export type RGB = [number, number, number];

export interface CartAPI {
  vox_set(x: number, y: number, z: number, color: number): void;
  vox_get(x: number, y: number, z: number): number;
  vox_clear(): void;
  vox_fill(x0: number, y0: number, z0: number, x1: number, y1: number, z1: number, color: number): void;
  vox_is_valid(x: number, y: number, z: number): boolean;

  neighbor(x: number, y: number, z: number, idx: number): [number, number, number];

  pal_set(slot: number, r: number, g: number, b: number): void;
  pal_reset(): void;

  cam_pitch(deg: number): void;
  cam_pitch_get(): number;

  btn(idx: number): boolean;
  btnp(idx: number): boolean;

  time(): number;
  rand(): number;

  print(...args: unknown[]): void;
}

export interface Cart {
  init(api: CartAPI): void;
  update(api: CartAPI, dt: number): void;
}

// Default 15-color palette (slot 0 = empty, not used).
const DEFAULT_PALETTE: RGB[] = [
  [0,    0,    0   ], //  0 empty
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

const BTN_MAP: Record<number, string[]> = {
  0: ['a'],            // left
  1: ['d'],            // right
  2: ['s'],            // down (-Z)
  3: ['w'],            // up   (+Z)
  4: ['shift'],        // descend
  5: [' '],            // ascend
  6: ['z'],            // A
  7: ['x'],            // B
  8: ['c'],            // C
  9: ['v'],            // D
};

export function startEmulator(container: HTMLElement, cart: Cart) {
  // ---- display buffer (1 byte per integer-lattice slot, 1/2 unused due to parity) ----
  const buffer = new Uint8Array(N3);

  // ---- three.js setup ----
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x101018);
  scene.fog = new THREE.Fog(0x101018, 180, 320);

  const camera = new THREE.PerspectiveCamera(
    35,
    container.clientWidth / container.clientHeight,
    0.5,
    1000,
  );

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setSize(container.clientWidth, container.clientHeight);
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  container.appendChild(renderer.domElement);

  scene.add(new THREE.AmbientLight(0xffffff, 0.55));
  const sun = new THREE.DirectionalLight(0xfff0d8, 0.8);
  sun.position.set(80, 150, 60);
  scene.add(sun);
  const fill = new THREE.DirectionalLight(0x88aaff, 0.25);
  fill.position.set(-50, -10, -40);
  scene.add(fill);

  // ---- RD geometry, instanced ----
  const rd = buildRdGeometry();
  const geom = new THREE.BufferGeometry();
  geom.setAttribute('position', new THREE.BufferAttribute(rd.positions, 3));
  geom.computeVertexNormals();

  const material = new THREE.MeshStandardMaterial({
    color: 0xffffff,
    roughness: 0.6,
    metalness: 0.05,
    flatShading: true,
  });

  const mesh = new THREE.InstancedMesh(geom, material, MAX_INSTANCES);
  mesh.count = 0;
  mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
  const colorArr = new Float32Array(MAX_INSTANCES * 3);
  const colorAttr = new THREE.InstancedBufferAttribute(colorArr, 3);
  colorAttr.setUsage(THREE.DynamicDrawUsage);
  mesh.instanceColor = colorAttr;
  scene.add(mesh);

  // ---- instance slot management ----
  // cell index → instance slot (only for non-empty cells)
  const cellToSlot = new Map<number, number>();
  const slotToCell: number[] = [];
  const tmpMatrix = new THREE.Matrix4();

  // ---- palette ----
  const palette: RGB[] = DEFAULT_PALETTE.map((c) => [...c] as RGB);

  function writeSlotMatrix(slot: number, x: number, y: number, z: number) {
    tmpMatrix.makeTranslation(x, y, z);
    mesh.setMatrixAt(slot, tmpMatrix);
  }
  function writeSlotColor(slot: number, color: number) {
    const [r, g, b] = palette[color];
    colorArr[slot * 3 + 0] = r;
    colorArr[slot * 3 + 1] = g;
    colorArr[slot * 3 + 2] = b;
  }

  function allocSlot(cellIdx: number, x: number, y: number, z: number, color: number) {
    const slot = slotToCell.length;
    if (slot >= MAX_INSTANCES) return;
    cellToSlot.set(cellIdx, slot);
    slotToCell.push(cellIdx);
    writeSlotMatrix(slot, x, y, z);
    writeSlotColor(slot, color);
    mesh.count = slotToCell.length;
  }

  function freeSlot(cellIdx: number) {
    const slot = cellToSlot.get(cellIdx);
    if (slot === undefined) return;
    const lastSlot = slotToCell.length - 1;
    if (slot !== lastSlot) {
      const lastIdx = slotToCell[lastSlot];
      slotToCell[slot] = lastIdx;
      cellToSlot.set(lastIdx, slot);
      const lz = (lastIdx / N2) | 0;
      const ly = ((lastIdx - lz * N2) / N) | 0;
      const lx = lastIdx - lz * N2 - ly * N;
      writeSlotMatrix(slot, lx, ly, lz);
      writeSlotColor(slot, buffer[lastIdx]);
    }
    slotToCell.pop();
    cellToSlot.delete(cellIdx);
    mesh.count = slotToCell.length;
  }

  let dirty = false;

  // ---- camera state ----
  let pitch = 30;       // [0, 90], cart-controlled
  let yawDev = 35;      // dev orbit yaw, not cart-visible
  let camDistance = 170;
  const camTarget = new THREE.Vector3(N / 2, N / 4, N / 2);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.minDistance = 20;
  controls.maxDistance = 400;
  controls.target.copy(camTarget);

  function placeCameraFromPitchYaw() {
    const pitchRad = (pitch * Math.PI) / 180;
    const yawRad = (yawDev * Math.PI) / 180;
    const horiz = Math.cos(pitchRad);
    const eye = new THREE.Vector3(
      camTarget.x + camDistance * horiz * Math.sin(yawRad),
      camTarget.y + camDistance * Math.sin(pitchRad),
      camTarget.z + camDistance * horiz * Math.cos(yawRad),
    );
    camera.position.copy(eye);
    camera.lookAt(camTarget);
  }
  placeCameraFromPitchYaw();

  // ---- input ----
  const keysDown = new Set<string>();
  const keysPressed = new Set<string>();

  window.addEventListener('keydown', (e) => {
    const k = e.key.toLowerCase();
    if (!keysDown.has(k)) keysPressed.add(k);
    keysDown.add(k);
  });
  window.addEventListener('keyup', (e) => {
    keysDown.delete(e.key.toLowerCase());
  });
  window.addEventListener('blur', () => {
    keysDown.clear();
    keysPressed.clear();
  });

  // ---- syscall API ----
  let tickCount = 0;

  const api: CartAPI = {
    vox_set(x, y, z, color) {
      x |= 0; y |= 0; z |= 0;
      if (x < 0 || x >= N || y < 0 || y >= N || z < 0 || z >= N) return;
      if (((x + y + z) & 1) !== 0) return;
      color = color & 0xf;
      const idx = x + y * N + z * N2;
      const prev = buffer[idx];
      if (prev === color) return;
      buffer[idx] = color;
      if (prev === 0 && color !== 0) {
        allocSlot(idx, x, y, z, color);
      } else if (prev !== 0 && color === 0) {
        freeSlot(idx);
      } else {
        const slot = cellToSlot.get(idx)!;
        writeSlotColor(slot, color);
      }
      dirty = true;
    },
    vox_get(x, y, z) {
      x |= 0; y |= 0; z |= 0;
      if (x < 0 || x >= N || y < 0 || y >= N || z < 0 || z >= N) return 0;
      return buffer[x + y * N + z * N2];
    },
    vox_clear() {
      buffer.fill(0);
      cellToSlot.clear();
      slotToCell.length = 0;
      mesh.count = 0;
      dirty = true;
    },
    vox_fill(x0, y0, z0, x1, y1, z1, color) {
      const lx = Math.max(0, Math.min(x0, x1) | 0);
      const ly = Math.max(0, Math.min(y0, y1) | 0);
      const lz = Math.max(0, Math.min(z0, z1) | 0);
      const ux = Math.min(N - 1, Math.max(x0, x1) | 0);
      const uy = Math.min(N - 1, Math.max(y0, y1) | 0);
      const uz = Math.min(N - 1, Math.max(z0, z1) | 0);
      for (let z = lz; z <= uz; z++) {
        for (let y = ly; y <= uy; y++) {
          for (let x = lx; x <= ux; x++) {
            this.vox_set(x, y, z, color);
          }
        }
      }
    },
    vox_is_valid(x, y, z) {
      x |= 0; y |= 0; z |= 0;
      return x >= 0 && x < N && y >= 0 && y < N && z >= 0 && z < N && ((x + y + z) & 1) === 0;
    },
    neighbor(x, y, z, idx) {
      const [dx, dy, dz] = NEIGHBOR_OFFSETS[idx & 0xf];
      return [x + dx, y + dy, z + dz];
    },
    pal_set(slot, r, g, b) {
      if (slot < 1 || slot > 15) return;
      palette[slot] = [r, g, b];
      // Repaint instances using this slot.
      for (let s = 0; s < slotToCell.length; s++) {
        if (buffer[slotToCell[s]] === slot) writeSlotColor(s, slot);
      }
      dirty = true;
    },
    pal_reset() {
      for (let i = 1; i < 16; i++) {
        const [r, g, b] = DEFAULT_PALETTE[i];
        api.pal_set(i, r, g, b);
      }
    },
    cam_pitch(deg) {
      pitch = Math.max(0, Math.min(90, deg));
    },
    cam_pitch_get() {
      return pitch;
    },
    btn(idx) {
      const keys = BTN_MAP[idx];
      return keys ? keys.some((k) => keysDown.has(k)) : false;
    },
    btnp(idx) {
      const keys = BTN_MAP[idx];
      return keys ? keys.some((k) => keysPressed.has(k)) : false;
    },
    time() {
      return tickCount;
    },
    rand() {
      return Math.random();
    },
    print(...args) {
      // eslint-disable-next-line no-console
      console.log('[cart]', ...args);
    },
  };

  // ---- HUD ----
  const hud = document.createElement('div');
  hud.style.cssText = [
    'position:fixed', 'top:8px', 'left:8px',
    'color:#dde', 'font-family:ui-monospace,monospace', 'font-size:12px',
    'background:rgba(0,0,0,0.55)', 'padding:8px 10px', 'border-radius:6px',
    'pointer-events:none', 'line-height:1.5', 'white-space:pre',
  ].join(';');
  document.body.appendChild(hud);

  let fpsAccum = 0;
  let fpsFrames = 0;
  let fpsValue = 0;

  function updateHud() {
    const filled = slotToCell.length;
    const cap = MAX_INSTANCES;
    const percent = ((filled / cap) * 100).toFixed(1);
    hud.textContent = [
      `omnivixion stub emulator`,
      `lattice:  rhombic dodecahedral (FCC), 128³`,
      `filled:   ${filled.toLocaleString()} / ${cap.toLocaleString()}  (${percent}%)`,
      `pitch:    ${pitch.toFixed(0)}°  (cart-controlled)`,
      `fps:      ${fpsValue.toFixed(0)}`,
      ``,
      `mouse drag  orbit (dev)   wheel  zoom`,
      `cart input  WASD + Space/Shift + ZXCV`,
    ].join('\n');
  }

  // ---- boot cart ----
  const tInit = performance.now();
  cart.init(api);
  // Push initial buffer state to GPU
  if (dirty) {
    mesh.instanceMatrix.needsUpdate = true;
    colorAttr.needsUpdate = true;
    dirty = false;
  }
  api.print(`init: ${(performance.now() - tInit).toFixed(0)}ms, ${slotToCell.length} cells`);

  // ---- resize ----
  window.addEventListener('resize', () => {
    camera.aspect = container.clientWidth / container.clientHeight;
    camera.updateProjectionMatrix();
    renderer.setSize(container.clientWidth, container.clientHeight);
  });

  // ---- main loop ----
  let last = performance.now();
  function frame() {
    requestAnimationFrame(frame);
    const now = performance.now();
    const dt = (now - last) / 1000;
    last = now;

    fpsAccum += dt;
    fpsFrames++;
    if (fpsAccum >= 0.5) {
      fpsValue = fpsFrames / fpsAccum;
      fpsAccum = 0;
      fpsFrames = 0;
    }

    cart.update(api, dt);
    tickCount++;
    keysPressed.clear();

    if (dirty) {
      mesh.instanceMatrix.needsUpdate = true;
      colorAttr.needsUpdate = true;
      dirty = false;
    }

    controls.update();
    updateHud();
    renderer.render(scene, camera);
  }
  frame();
}
