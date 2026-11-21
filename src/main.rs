//! Proyecto 3 — Sistema solar "Aurelia" con un software renderer en CPU.
//!
//! Ejecutar:        cargo run --release
//! Recorrido demo:  cargo run --release -- --demo docs/demo.gif

mod camera;
mod demo;
mod framebuffer;
mod mesh;
mod noise;
mod render;
mod scene;
mod shaders;
mod ship;
mod skybox;
mod vertex;

use camera::{Camera, CameraMode};
use framebuffer::Framebuffer;
use minifb::{Key, KeyRepeat, Window, WindowOptions};
use render::{Meshes, Params, Stats, Uniforms};
use scene::BODIES;
use ship::{Input, Ship};
use std::time::Instant;

pub const WIDTH: usize = 960;
pub const HEIGHT: usize = 540;

/// Estado completo de la simulación.
pub struct State {
    /// Tiempo del sistema (controla órbitas y rotaciones; no depende de los FPS).
    pub time: f32,
    pub time_scale: f32,
    pub ship: Ship,
    pub camera: Camera,
    pub show_orbits: bool,
    pub params: Params,
}

impl State {
    pub fn new() -> Self {
        let ship = Ship::new(0.0);
        let camera = Camera::new(&ship);
        State {
            time: 0.0,
            time_scale: 1.0,
            ship,
            camera,
            show_orbits: true,
            params: Params::default(),
        }
    }

    /// Avanza la simulación `dt` segundos reales.
    pub fn step(&mut self, input: &Input, dt: f32) {
        let t0 = self.time;
        self.time += dt * self.time_scale;
        self.ship.update(input, dt, t0, self.time);
        self.camera.update(&self.ship, dt, self.time);
    }

    pub fn uniforms(&self, width: usize, height: usize) -> Uniforms {
        let (fwd, right, up) = self.camera.basis();
        let (w, h) = (width as f32, height as f32);
        Uniforms {
            view: self.camera.view_matrix(),
            projection: render::projection_matrix(w, h, self.camera.fov),
            width: w,
            height: h,
            time: self.time,
            cam_pos: self.camera.eye,
            cam_fwd: fwd,
            cam_right: right,
            cam_up: up,
            tan_half_fov: (self.camera.fov * 0.5).tan(),
            light_pos: nalgebra_glm::Vec3::zeros(),
            params: self.params,
            warp: self.ship.warp_intensity(),
            thrust: self.ship.throttle,
            show_orbits: self.show_orbits,
        }
    }

    /// Dibuja el cuadro actual (cuerpos del sistema + la nave).
    pub fn draw(&self, fb: &mut Framebuffer, meshes: &Meshes, noise: &noise::Noises) -> Stats {
        let mut instances = scene::build(self.time);
        instances.push(self.ship.instance());
        render::render(fb, meshes, &instances, &self.uniforms(fb.width, fb.height), noise)
    }
}

pub fn load_meshes() -> Meshes {
    let ship = mesh::load_obj("assets/ship.obj", |name| {
        if name.starts_with("cabina") {
            1
        } else if name.starts_with("ala") || name.starts_with("aleta") {
            2
        } else if name.starts_with("motor") {
            3
        } else if name.starts_with("tobera") {
            4
        } else {
            0
        }
    })
    .expect("No se encontró assets/ship.obj (ejecuta desde la raíz del proyecto)");
    Meshes {
        // Niveles de detalle: 320, 1280, 5120 y 20480 triángulos.
        spheres: (2..=5).map(mesh::icosphere).collect(),
        ship,
    }
}

fn main() {
    let meshes = load_meshes();
    let noise = noise::Noises::new();

    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--demo") {
        let out = args.get(i + 1).cloned().unwrap_or_else(|| "frames".into());
        demo::run(&out, &meshes, &noise);
        return;
    }

    let mut fb = Framebuffer::new(WIDTH, HEIGHT);
    let mut window = Window::new(
        "Sistema solar Aurelia",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )
    .expect("No se pudo crear la ventana");

    let mut state = State::new();
    let mut paused = false;
    let mut last = Instant::now();
    let mut fps_timer = 0.0;
    let mut frames = 0;
    let mut stats = Stats::default();
    print_help();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();

        let input = read_input(&window, &mut state, &mut paused);
        state.step(&input, if paused { 0.0 } else { dt });
        if paused {
            // En pausa la cámara se sigue actualizando para poder cambiar de modo.
            state.camera.update(&state.ship, dt, state.time);
        }

        stats = state.draw(&mut fb, &meshes, &noise);
        window.update_with_buffer(&fb.color, WIDTH, HEIGHT).unwrap();

        frames += 1;
        fps_timer += dt;
        if fps_timer > 0.5 {
            let (near, dist) = ship::nearest_body(state.ship.pos, state.time);
            let status = if let Some(w) = &state.ship.warp {
                format!("WARP → {}", BODIES[w.target].name)
            } else {
                format!("cerca de {} ({:.1})", BODIES[near].name, dist.max(0.0))
            };
            window.set_title(&format!(
                "Aurelia | {} | vel {:.1} | cámara: {} | x{:.1}{} | {:.0} fps | {} tris, {} descartados",
                status,
                state.ship.speed(),
                state.camera.mode.name(),
                state.time_scale,
                if paused { " (pausa)" } else { "" },
                frames as f32 / fps_timer,
                stats.triangles,
                stats.culled,
            ));
            frames = 0;
            fps_timer = 0.0;
        }
    }
    let _ = stats;
}

/// Traduce el teclado a una `Input` para la nave y aplica los atajos globales.
fn read_input(window: &Window, s: &mut State, paused: &mut bool) -> Input {
    let down = |k| window.is_key_down(k);
    let pressed = |k| window.is_key_pressed(k, KeyRepeat::No);
    let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32;

    let mut input = Input {
        thrust: axis(down(Key::S), down(Key::W)),
        turn: axis(down(Key::A), down(Key::D)),
        pitch: axis(down(Key::Down), down(Key::Up)),
        lift: axis(down(Key::Q) || down(Key::LeftCtrl), down(Key::E) || down(Key::Space)),
        boost: down(Key::LeftShift) || down(Key::RightShift),
        brake: down(Key::X),
        warp_to: None,
    };
    if down(Key::Left) {
        input.turn = -1.0;
    }
    if down(Key::Right) {
        input.turn = 1.0;
    }

    let warp_keys = [
        Key::Key1,
        Key::Key2,
        Key::Key3,
        Key::Key4,
        Key::Key5,
        Key::Key6,
        Key::Key7,
    ];
    for (i, k) in warp_keys.iter().enumerate() {
        if pressed(*k) {
            input.warp_to = Some(i);
        }
    }

    if pressed(Key::C) {
        s.camera.mode = s.camera.mode.next();
    }
    if s.camera.mode == CameraMode::Overview {
        let dt = 1.0 / 60.0;
        if down(Key::J) {
            s.camera.overview_angle -= dt;
        }
        if down(Key::L) {
            s.camera.overview_angle += dt;
        }
        if down(Key::I) {
            s.camera.overview_height = (s.camera.overview_height - 30.0 * dt).max(15.0);
        }
        if down(Key::K) {
            s.camera.overview_height = (s.camera.overview_height + 30.0 * dt).min(140.0);
        }
    }
    if pressed(Key::O) {
        s.show_orbits = !s.show_orbits;
    }
    if pressed(Key::P) {
        *paused = !*paused;
    }
    if pressed(Key::Equal) {
        s.time_scale = (s.time_scale * 2.0).min(16.0);
    }
    if pressed(Key::Minus) {
        s.time_scale = (s.time_scale * 0.5).max(0.125);
    }
    if pressed(Key::R) {
        let mode = s.camera.mode;
        *s = State::new();
        s.camera.mode = mode;
    }
    input
}

fn print_help() {
    println!("=== Sistema solar Aurelia (software renderer) ===");
    println!("W / S: acelerar / retroceder    A D o ← →: girar    ↑ ↓: cabeceo (nariz)");
    println!("E o Espacio / Q o Ctrl: subir / bajar    Shift: turbo    X: frenar");
    println!("1-7: warp a Helios, Ignis, Terra Nova, Selene, Nyx, Zephyr, Glacies");
    println!("C: modo de cámara (persecución / cinemática / vista general)");
    println!("   vista general: J L girar, I K acercar / alejar");
    println!("O: órbitas   - =: velocidad del tiempo   P: pausa   R: reiniciar   Esc: salir");
}
