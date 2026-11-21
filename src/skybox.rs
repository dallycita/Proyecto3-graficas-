//! Skybox procedural: se evalúa con la DIRECCIÓN del rayo de cada píxel, así
//! las estrellas quedan "en el infinito" y se mueven correctamente cuando la
//! cámara gira (a diferencia de un fondo en coordenadas de pantalla).
//!
//! La dirección se proyecta sobre las 6 caras de un cubo (como un cube map);
//! cada cara se divide en una cuadrícula y cada celda puede contener una
//! estrella con posición, brillo y color pseudoaleatorios.
//! Capas:
//!   1. Estrellas tenues y densas.
//!   2. Estrellas brillantes y escasas con tinte de color y centelleo.
//!   3. Banda galáctica (una "vía láctea") alrededor de un círculo máximo.
//!   4. Nebulosas suaves de baja frecuencia.

use crate::noise::*;
use crate::render::Uniforms;
use crate::shaders::common::*;
use nalgebra_glm::Vec3;

/// Proyección cúbica: devuelve (cara, u, v) con u, v en [-1, 1].
fn cube_face(d: Vec3) -> (u32, f32, f32) {
    let a = d.abs();
    if a.x >= a.y && a.x >= a.z {
        let f = if d.x > 0.0 { 0 } else { 1 };
        (f, d.y / a.x, d.z / a.x)
    } else if a.y >= a.z {
        let f = if d.y > 0.0 { 2 } else { 3 };
        (f, d.x / a.y, d.z / a.y)
    } else {
        let f = if d.z > 0.0 { 4 } else { 5 };
        (f, d.x / a.z, d.y / a.z)
    }
}

/// Una capa de estrellas: `cells` celdas por lado de cada cara del cubo.
fn star_layer(d: Vec3, cells: f32, density: f32, size: f32, seed: f32, time: f32) -> Vec3 {
    let (face, u, v) = cube_face(d);
    let gu = (u * 0.5 + 0.5) * cells;
    let gv = (v * 0.5 + 0.5) * cells;
    let (cu, cv) = (gu.floor(), gv.floor());
    let id_x = cu + face as f32 * 1031.0 + seed;
    let id_y = cv + seed * 3.7;
    let h = hash2(id_x, id_y);
    if h > density {
        return Vec3::zeros();
    }
    // Posición de la estrella dentro de la celda (lejos de los bordes).
    let ox = 0.25 + 0.5 * hash2(id_y + 5.1, id_x);
    let oy = 0.25 + 0.5 * hash2(id_x - 2.3, id_y + 9.7);
    let dist = ((gu - cu - ox).powi(2) + (gv - cv - oy).powi(2)).sqrt();
    let core = (-(dist / size).powi(2)).exp();
    let bright = 0.35 + 0.65 * hash2(id_x * 1.7, id_y * 0.3);
    let twinkle = 0.75 + 0.25 * (time * (1.5 + 3.0 * h) + h * 40.0).sin();
    let tint = ramp(
        &[
            (0.0, hex(0x9db8ff)),
            (0.45, hex(0xffffff)),
            (0.8, hex(0xffe7b0)),
            (1.0, hex(0xffa070)),
        ],
        hash2(id_y, id_x + 3.3),
    );
    tint * core * bright * twinkle
}

pub fn sample(d: Vec3, u: &Uniforms, nz: &Noises) -> Vec3 {
    let mut col = hex(0x020309);

    // 3. Banda galáctica: inclinada respecto a la eclíptica.
    let band_normal = Vec3::new(0.25, 0.92, 0.3).normalize();
    let band = (-(d.dot(&band_normal) / 0.22).powi(2)).exp();
    let dust = n01(&nz.warp, d * 3.0);
    col += mix(hex(0x1a1430), hex(0x3a3050), dust) * band * (0.55 + 0.6 * dust);

    // 4. Nebulosas.
    let neb = n01(&nz.simplex, d * 1.6 + Vec3::new(3.0, 0.0, 1.0));
    let neb2 = n01(&nz.simplex, d * 2.3 - Vec3::new(1.0, 4.0, 0.0));
    col += hex(0x3b0f4a) * smoothstep(0.6, 0.95, neb) * 0.3;
    col += hex(0x0b3b4a) * smoothstep(0.65, 0.95, neb2) * 0.25;

    // 1-2. Estrellas (más densas dentro de la banda galáctica).
    let t = u.time;
    col += star_layer(d, 420.0, 0.10 + 0.12 * band, 0.22, 0.0, t) * 0.8;
    col += star_layer(d, 140.0, 0.07, 0.09, 17.0, t) * 1.4;
    col
}
