//! Recorrido automático sin ventana. Usa EXACTAMENTE la misma simulación que
//! el modo interactivo, pero las entradas de la nave salen de un guion.
//!
//!   cargo run --release -- --demo docs/demo.gif     (GIF reducido para el README)
//!   cargo run --release -- --demo frames            (cuadros PNG a resolución completa)
//!   cargo run --release -- --demo frames 12.5       (un solo cuadro en el segundo 12.5)

use crate::camera::CameraMode;
use crate::framebuffer::Framebuffer;
use crate::noise::Noises;
use crate::render::Meshes;
use crate::ship::Input;
use crate::{State, HEIGHT, WIDTH};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{imageops, Delay, Frame, RgbaImage};
use std::fs::{self, File};

const FPS: f32 = 30.0;

/// Un tramo del recorrido: duración, entrada constante, modo de cámara y warp opcional al inicio.
struct Segment {
    secs: f32,
    input: Input,
    camera: CameraMode,
    warp: Option<usize>,
}

fn seg(secs: f32, camera: CameraMode, warp: Option<usize>, f: impl Fn(&mut Input)) -> Segment {
    let mut input = Input::default();
    f(&mut input);
    Segment {
        secs,
        input,
        camera,
        warp,
    }
}

fn script() -> Vec<Segment> {
    use CameraMode::*;
    vec![
        seg(3.5, Overview, None, |_| {}),
        seg(3.0, Chase, None, |i| i.thrust = 1.0),
        seg(4.0, Chase, None, |i| {
            i.thrust = 0.5;
            i.turn = 0.6;
        }),
        seg(3.0, Chase, Some(3), |_| {}), // Selene
        seg(3.0, Cinematic, None, |_| {}),
        seg(3.0, Chase, Some(5), |_| {}), // Zephyr
        seg(4.0, Chase, None, |i| {
            i.thrust = 0.6;
            i.turn = -0.5;
            i.lift = 0.4;
        }),
        seg(3.0, Chase, Some(4), |_| {}), // Nyx
        seg(3.5, Cinematic, None, |_| {}),
        seg(3.0, Chase, Some(1), |_| {}), // Ignis
        seg(3.0, Chase, None, |i| i.thrust = 1.0), // hasta chocar: la colisión lo detiene
        seg(3.0, Chase, Some(6), |_| {}), // Glacies
        seg(3.0, Chase, None, |i| {
            i.thrust = 0.4;
            i.turn = 0.7;
            i.pitch = 0.2;
        }),
        seg(3.0, Chase, Some(0), |_| {}), // Helios
        seg(3.0, Chase, None, |i| i.turn = -0.4),
        seg(4.0, Overview, None, |_| {}),
    ]
}

pub fn run(out: &str, meshes: &Meshes, noise: &Noises) {
    let args: Vec<String> = std::env::args().collect();
    let single: Option<f32> = args.last().and_then(|a| a.parse().ok());
    let gif = out.ends_with(".gif");

    let mut fb = Framebuffer::new(WIDTH, HEIGHT);
    let mut state = State::new();
    let dt = 1.0 / FPS;

    let mut encoder = if gif {
        if let Some(dir) = std::path::Path::new(out).parent() {
            fs::create_dir_all(dir).ok();
        }
        let mut e = GifEncoder::new_with_speed(File::create(out).expect("no se pudo crear el GIF"), 20);
        e.set_repeat(Repeat::Infinite).unwrap();
        Some(e)
    } else {
        fs::create_dir_all(out).unwrap();
        None
    };

    let mut frame = 0usize;
    let mut elapsed = 0.0;
    for s in script() {
        state.camera.mode = s.camera;
        let mut first = true;
        let n = (s.secs * FPS).round() as usize;
        for _ in 0..n {
            let mut input = s.input;
            if first {
                input.warp_to = s.warp;
                first = false;
            }
            state.step(&input, dt);
            elapsed += dt;

            if let Some(t) = single {
                if (elapsed - t).abs() < dt * 0.5 {
                    state.draw(&mut fb, meshes, noise);
                    fb.save_png(&format!("{out}/still.png"));
                    println!("{out}/still.png");
                    return;
                }
                continue;
            }

            if let Some(e) = encoder.as_mut() {
                // GIF: 6 cuadros por segundo a un tercio de la resolución (para que pese poco).
                if frame % 5 == 0 {
                    state.draw(&mut fb, meshes, noise);
                    let img = fb_to_rgba(&fb);
                    let small = imageops::resize(&img, WIDTH as u32 / 3, HEIGHT as u32 / 3, imageops::FilterType::Triangle);
                    e.encode_frame(Frame::from_parts(small, 0, 0, Delay::from_numer_denom_ms(166, 1)))
                        .unwrap();
                }
            } else {
                state.draw(&mut fb, meshes, noise);
                fb.save_png(&format!("{out}/f{frame:05}.png"));
            }
            frame += 1;
            if frame % 60 == 0 {
                println!("  {:.0} s", elapsed);
            }
        }
    }
    println!("Listo: {frame} cuadros ({out})");
}

fn fb_to_rgba(fb: &Framebuffer) -> RgbaImage {
    let mut img = RgbaImage::new(fb.width as u32, fb.height as u32);
    for (i, c) in fb.color.iter().enumerate() {
        img.put_pixel(
            (i % fb.width) as u32,
            (i / fb.width) as u32,
            image::Rgba([(c >> 16) as u8, (c >> 8) as u8, *c as u8, 255]),
        );
    }
    img
}
