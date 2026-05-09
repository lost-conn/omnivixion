import { startEmulator } from './emulator';
import { cart } from './cart';

const container = document.getElementById('app');
if (!container) throw new Error('#app missing');

startEmulator(container, cart);
