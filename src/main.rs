mod cart;
mod console;
mod font;
mod lattice;
mod render;

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use glam::Vec3;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use cart::{Cart, DemoCart, PacmanCart};
use console::Console;
use lattice::N;
use render::Renderer;

struct CameraOrbit {
    target: Vec3,
    distance: f32,
    yaw_deg: f32,
    pitch_deg: f32,
    drag: bool,
    last_mouse: Option<(f32, f32)>,
}

impl CameraOrbit {
    fn new() -> Self {
        let n = N as f32;
        Self {
            target: Vec3::new(n * 0.5, n * 0.25, n * 0.5),
            distance: 170.0,
            yaw_deg: 0.0,
            pitch_deg: 30.0,
            drag: false,
            last_mouse: None,
        }
    }

    fn eye(&self) -> Vec3 {
        let pitch = self.pitch_deg.to_radians();
        let yaw = self.yaw_deg.to_radians();
        let horiz = pitch.cos();
        Vec3::new(
            self.target.x + self.distance * horiz * yaw.sin(),
            self.target.y + self.distance * pitch.sin(),
            self.target.z + self.distance * horiz * yaw.cos(),
        )
    }
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    console: Console,
    cart: Box<dyn Cart>,
    orbit: CameraOrbit,
    last_frame: Instant,
    cart_inited: bool,
    update_accum: f32,
}

const LOGIC_HZ: f32 = 60.0;
const LOGIC_DT: f32 = 1.0 / LOGIC_HZ;
const MAX_UPDATES_PER_FRAME: u32 = 5; // avoid spiral of death after long stalls

impl App {
    fn new() -> Self {
        let cart: Box<dyn Cart> = if std::env::args().any(|a| a == "--demo") {
            Box::new(DemoCart::new())
        } else {
            Box::new(PacmanCart::new())
        };
        Self {
            window: None,
            renderer: None,
            console: Console::new(),
            cart,
            orbit: CameraOrbit::new(),
            last_frame: Instant::now(),
            cart_inited: false,
            update_accum: 0.0,
        }
    }

    fn frame(&mut self) -> Result<()> {
        let Some(renderer) = self.renderer.as_mut() else { return Ok(()) };

        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;

        if !self.cart_inited {
            let t0 = Instant::now();
            self.cart.init(&mut self.console);
            let elapsed = t0.elapsed().as_millis();
            println!(
                "[emulator] cart init: {} ms, {} visible cells",
                elapsed,
                self.console.instances.len()
            );
            self.cart_inited = true;
            self.update_accum = 0.0;

            // Auto-fit the camera to the populated region the cart wrote.
            // (Spec: emulator owns viewport. Carts can hint pitch but not size.)
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for inst in &self.console.instances {
                let p = Vec3::from(inst.pos);
                min = min.min(p);
                max = max.max(p);
            }
            if max.x.is_finite() && max.x >= min.x {
                let center = (min + max) * 0.5;
                let extent = (max - min).max_element().max(1.0);
                self.orbit.target = center;
                self.orbit.distance = (extent * 1.4).clamp(20.0, 400.0);
            }
        }

        // Fixed-step logic update so cart speed is independent of render rate.
        self.update_accum += dt;
        let mut steps = 0u32;
        while self.update_accum >= LOGIC_DT && steps < MAX_UPDATES_PER_FRAME {
            self.cart.update(&mut self.console, LOGIC_DT);
            self.console.tick = self.console.tick.wrapping_add(1);
            // Clear edge-trigger pressed-this-frame state after each logic tick so
            // a single keypress fires exactly once.
            self.console.keys_pressed.clear();
            self.update_accum -= LOGIC_DT;
            steps += 1;
        }
        // Drop accumulated time from a long stall (window unfocus, etc.).
        if self.update_accum > LOGIC_DT * MAX_UPDATES_PER_FRAME as f32 {
            self.update_accum = 0.0;
        }

        // Camera pitch follows the cart's requested pitch when the user isn't dragging.
        // Time-constant ~150ms — settles in under a second with no overshoot.
        if !self.orbit.drag {
            let t = 1.0 - (-dt / 0.15).exp();
            self.orbit.pitch_deg += (self.console.pitch - self.orbit.pitch_deg) * t;
        }

        renderer.upload_instances(&mut self.console);
        renderer.render(&self.console, self.orbit.eye(), self.orbit.target)?;

        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("omnivixion")
            .with_inner_size(PhysicalSize::new(1280u32, 800u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let renderer = pollster::block_on(Renderer::new(window.clone()))
            .expect("renderer init");
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    match event.state {
                        ElementState::Pressed => {
                            if !self.console.keys_down.contains(&code) {
                                self.console.keys_pressed.insert(code);
                            }
                            self.console.keys_down.insert(code);
                            if code == KeyCode::Escape {
                                event_loop.exit();
                            }
                        }
                        ElementState::Released => {
                            self.console.keys_down.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    self.orbit.drag = matches!(state, ElementState::Pressed);
                    if !self.orbit.drag {
                        self.orbit.last_mouse = None;
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (px, py) = (position.x as f32, position.y as f32);
                if let Some((lx, ly)) = self.orbit.last_mouse {
                    let dx = px - lx;
                    let dy = py - ly;
                    if self.orbit.drag {
                        self.orbit.yaw_deg -= dx * 0.4;
                        self.orbit.pitch_deg = (self.orbit.pitch_deg + dy * 0.3)
                            .clamp(2.0, 88.0);
                    }
                }
                self.orbit.last_mouse = Some((px, py));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 8.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                self.orbit.distance = (self.orbit.distance - scroll).clamp(20.0, 400.0);
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.frame() {
                    eprintln!("frame error: {e:#}");
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
