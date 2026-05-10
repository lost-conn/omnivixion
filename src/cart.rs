//! Cart trait. Implementations live in `loader.rs` (`LuaCart`) and load
//! from `.omni` text files in `carts/`.

use crate::console::CartApi;

pub trait Cart {
    fn init(&mut self, api: &mut dyn CartApi);
    fn update(&mut self, api: &mut dyn CartApi, dt: f32);
}
