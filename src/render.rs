//! Pipeline del software renderer:
//!   1. Descarte de objetos fuera de la vista (frustum culling por esfera envolvente).
//!   2. Selección del nivel de detalle de la esfera según su tamaño en pantalla.
//!   3. Vertex shader (por vértice) -> espacio de recorte.
//!   4. Recorte de triángulos contra el plano cercano (clip space).
//!   5. Rasterización con coordenadas baricéntricas + z-buffer.
//!   6. Fragment shader (una vez por píxel visible, en paralelo con rayon).
//!   7. Fondo: skybox de estrellas + halo del sol.
//!   8. Superposiciones: líneas de órbita y efecto de warp.

use crate::framebuffer::{to_u32, Fragment, Framebuffer, NO_INSTANCE};
use crate::mesh::Mesh;
use crate::noise::Noises;
use crate::scene::{Instance, MeshKind, ShaderKind, BODIES};
use crate::shaders::{self, common::*};
use crate::skybox;
use crate::vertex::{ClipVertex, ScreenVertex, Vertex};
use nalgebra_glm::{Mat4, Vec3, Vec4};
use rayon::prelude::*;
use std::f32::consts::TAU;

/// Parámetros ajustables en tiempo real que leen los shaders de los planetas.
#[derive(Clone, Copy)]
pub struct Params {
    pub cloud_coverage: f32,
    pub bump_strength: f32,
    pub parallax_scale: f32,
    pub storm_strength: f32,
    pub clouds: bool,
    pub cloud_shadows: bool,
    pub bump: bool,
    pub parallax: bool,
    pub storm: bool,
    pub night_lights: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            cloud_coverage: 0.5,
            bump_strength: 1.0,
            parallax_scale: 0.05,
            storm_strength: 4.0,
            clouds: true,
            cloud_shadows: true,
            bump: true,
            parallax: true,
            storm: true,
            night_lights: true,
        }
    }
}

pub struct Uniforms {
    pub view: Mat4,
    pub projection: Mat4,
    pub width: f32,
    pub height: f32,
    pub time: f32,
    pub cam_pos: Vec3,
    /// Base de la cámara (para reconstruir la dirección de cada píxel del skybox).
    pub cam_fwd: Vec3,
    pub cam_right: Vec3,
    pub cam_up: Vec3,
    pub tan_half_fov: f32,
    pub light_pos: Vec3,
    pub params: Params,
    /// Intensidad del efecto de warp (0 = sin warp, 1 = máximo).
    pub warp: f32,
    /// Potencia de los motores de la nave (brillo de las toberas).
    pub thrust: f32,
    pub show_orbits: bool,
}

/// Mallas disponibles: icoesferas en varios niveles de detalle y la nave.
pub struct Meshes {
    pub spheres: Vec<Mesh>,
    pub ship: Mesh,
}

/// Estadísticas del cuadro (se muestran en el título de la ventana).
#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub triangles: usize,
    pub culled: usize,
}

const NEAR: f32 = 0.05;
const FAR: f32 = 600.0;

pub fn projection_matrix(width: f32, height: f32, fov: f32) -> Mat4 {
    nalgebra_glm::perspective(width / height, fov, NEAR, FAR)
}

fn vertex_stage(v: &Vertex, inst: &Instance, vp: &Mat4) -> ClipVertex {
    let local = shaders::vertex_shader(v, inst);
    let world4 = inst.model * Vec4::new(local.x, local.y, local.z, 1.0);
    ClipVertex {
        clip: vp * world4,
        world: world4.xyz(),
        obj: v.position,
        normal: v.normal,
    }
}

/// Recorta un triángulo contra el plano cercano (z_clip >= -w_clip).
/// Produce 0, 1 o 2 triángulos.
fn clip_near(tri: [ClipVertex; 3], out: &mut Vec<[ClipVertex; 3]>) {
    let d = |v: &ClipVertex| v.clip.z + v.clip.w;
    let inside = tri.iter().filter(|v| d(v) >= 0.0).count();
    match inside {
        3 => out.push(tri),
        0 => {}
        _ => {
            // Sutherland-Hodgman contra un solo plano.
            let mut poly: Vec<ClipVertex> = Vec::with_capacity(4);
            for i in 0..3 {
                let a = &tri[i];
                let b = &tri[(i + 1) % 3];
                let (da, db) = (d(a), d(b));
                if da >= 0.0 {
                    poly.push(*a);
                }
                if (da >= 0.0) != (db >= 0.0) {
                    poly.push(a.lerp(b, da / (da - db)));
                }
            }
            for i in 1..poly.len() - 1 {
                out.push([poly[0], poly[i], poly[i + 1]]);
            }
        }
    }
}

fn to_screen(v: &ClipVertex, u: &Uniforms) -> ScreenVertex {
    let inv_w = 1.0 / v.clip.w;
    ScreenVertex {
        screen: Vec3::new(
            (v.clip.x * inv_w + 1.0) * 0.5 * u.width,
            (1.0 - v.clip.y * inv_w) * 0.5 * u.height,
            v.clip.z * inv_w,
        ),
        inv_w,
        world: v.world,
        obj: v.obj,
        normal: v.normal,
    }
}

#[inline]
fn edge(a: &Vec3, b: &Vec3, px: f32, py: f32) -> f32 {
    (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x)
}

fn rasterize(fb: &mut Framebuffer, t: [ScreenVertex; 3], id: u16, part: u8, inst: &Instance) {
    let (a, b, c) = (&t[0].screen, &t[1].screen, &t[2].screen);
    let area = edge(a, b, c.x, c.y);
    // Descarte de caras traseras: las mallas son cerradas, así que las caras
    // que miran hacia atrás nunca se ven (los anillos también son una esfera cerrada).
    // Las caras frontales están en sentido antihorario en el mundo; al invertir
    // el eje Y de la pantalla quedan con área negativa.
    if area >= -1e-8 {
        return;
    }
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
    let max_x = a.x.max(b.x).max(c.x).ceil().min(fb.width as f32 - 1.0) as i32;
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i32;
    let max_y = a.y.max(b.y).max(c.y).ceil().min(fb.height as f32 - 1.0) as i32;

    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let w0 = edge(b, c, px, py) / area;
            let w1 = edge(c, a, px, py) / area;
            let w2 = edge(a, b, px, py) / area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let depth = w0 * a.z + w1 * b.z + w2 * c.z;
            let idx = y as usize * fb.width + x as usize;
            if depth >= fb.fragments[idx].depth || depth < -1.0 {
                continue;
            }
            // Interpolación con corrección de perspectiva.
            let p0 = w0 * t[0].inv_w;
            let p1 = w1 * t[1].inv_w;
            let p2 = w2 * t[2].inv_w;
            let s = 1.0 / (p0 + p1 + p2);
            let obj = (t[0].obj * p0 + t[1].obj * p1 + t[2].obj * p2) * s;
            if shaders::discard(inst, &obj) {
                continue;
            }
            let world = (t[0].world * p0 + t[1].world * p1 + t[2].world * p2) * s;
            let normal = (t[0].normal * p0 + t[1].normal * p1 + t[2].normal * p2) * s;
            fb.fragments[idx] = Fragment {
                depth,
                world,
                obj,
                normal,
                part,
                instance: id,
            };
        }
    }
}

/// ¿La esfera envolvente de la instancia queda completamente fuera de la vista?
fn outside_frustum(inst: &Instance, u: &Uniforms) -> bool {
    let rel = inst.center - u.cam_pos;
    let z = rel.dot(&u.cam_fwd);
    let r = inst.radius * 1.05;
    if z + r < NEAR {
        return true; // detrás de la cámara
    }
    let aspect = u.width / u.height;
    let tx = u.tan_half_fov * aspect;
    let ty = u.tan_half_fov;
    // Distancia a los planos laterales (r / cos del semiángulo).
    let sx = r * (1.0 + tx * tx).sqrt();
    let sy = r * (1.0 + ty * ty).sqrt();
    rel.dot(&u.cam_right).abs() > z * tx + sx || rel.dot(&u.cam_up).abs() > z * ty + sy
}

/// Nivel de detalle según el radio aproximado en píxeles.
fn lod_level(inst: &Instance, u: &Uniforms, levels: usize) -> usize {
    let dist = (inst.center - u.cam_pos).norm().max(1e-3);
    let px = inst.radius / dist * (u.height * 0.5) / u.tan_half_fov;
    let lvl = if px < 12.0 {
        0
    } else if px < 45.0 {
        1
    } else if px < 160.0 {
        2
    } else {
        3
    };
    lvl.min(levels - 1)
}

pub fn render(
    fb: &mut Framebuffer,
    meshes: &Meshes,
    instances: &[Instance],
    u: &Uniforms,
    noise: &Noises,
) -> Stats {
    fb.clear();
    let mut stats = Stats::default();
    let vp = u.projection * u.view;
    let mut clipped: Vec<[ClipVertex; 3]> = Vec::new();

    // Etapas 1-5.
    for (id, inst) in instances.iter().enumerate() {
        if outside_frustum(inst, u) {
            stats.culled += 1;
            continue;
        }
        let mesh = match inst.mesh {
            MeshKind::Sphere => &meshes.spheres[lod_level(inst, u, meshes.spheres.len())],
            MeshKind::Ship => &meshes.ship,
        };
        let transformed: Vec<ClipVertex> = mesh
            .vertices
            .iter()
            .map(|v| vertex_stage(v, inst, &vp))
            .collect();
        for (tri, src) in transformed
            .chunks_exact(3)
            .zip(mesh.vertices.chunks_exact(3))
        {
            clipped.clear();
            clip_near([tri[0], tri[1], tri[2]], &mut clipped);
            for c in &clipped {
                let s = [to_screen(&c[0], u), to_screen(&c[1], u), to_screen(&c[2], u)];
                rasterize(fb, s, id as u16, src[0].part, inst);
                stats.triangles += 1;
            }
        }
    }

    // Halo del sol (solo si está delante de la cámara).
    let halos: Vec<(f32, f32, f32)> = instances
        .iter()
        .filter(|i| i.kind == ShaderKind::Star)
        .filter(|i| (i.center - u.cam_pos).dot(&u.cam_fwd) > NEAR)
        .map(|i| screen_circle(i, u))
        .collect();

    // Etapas 6-7: fragment shader / skybox en paralelo, una vez por píxel.
    let w = fb.width;
    let frags = &fb.fragments;
    fb.color.par_iter_mut().enumerate().for_each(|(i, out)| {
        let f = &frags[i];
        let (x, y) = ((i % w) as f32, (i / w) as f32);
        let color = if f.instance == NO_INSTANCE {
            let d = pixel_dir(x + 0.5, y + 0.5, u);
            skybox::sample(d, u, noise) + halo(x, y, &halos, u.time)
        } else {
            let inst = &instances[f.instance as usize];
            shaders::fragment_shader(f, inst, instances, u, noise)
        };
        *out = to_u32(color);
    });

    // Etapa 8: superposiciones.
    if u.show_orbits {
        draw_orbits(fb, u);
    }
    if u.warp > 0.001 {
        warp_effect(fb, u);
    }
    stats
}

/// Dirección del rayo de la cámara que pasa por el píxel (x, y).
fn pixel_dir(x: f32, y: f32, u: &Uniforms) -> Vec3 {
    let nx = (x / u.width) * 2.0 - 1.0;
    let ny = 1.0 - (y / u.height) * 2.0;
    let aspect = u.width / u.height;
    (u.cam_fwd + u.cam_right * (nx * u.tan_half_fov * aspect) + u.cam_up * (ny * u.tan_half_fov))
        .normalize()
}

/// Proyecta un punto del mundo a pantalla. None si está detrás de la cámara.
fn project(p: Vec3, u: &Uniforms) -> Option<Vec3> {
    let c = u.projection * u.view * Vec4::new(p.x, p.y, p.z, 1.0);
    if c.w < NEAR {
        return None;
    }
    Some(Vec3::new(
        (c.x / c.w + 1.0) * 0.5 * u.width,
        (1.0 - c.y / c.w) * 0.5 * u.height,
        c.z / c.w,
    ))
}

/// Centro y radio aproximado en pantalla de una esfera.
fn screen_circle(inst: &Instance, u: &Uniforms) -> (f32, f32, f32) {
    let c = project(inst.center, u).unwrap_or(Vec3::new(-1e4, -1e4, 0.0));
    let e = project(inst.center + u.cam_right * inst.radius, u).unwrap_or(c);
    (c.x, c.y, (e - c).xy().norm())
}

/// Corona del sol sobre el fondo.
fn halo(x: f32, y: f32, halos: &[(f32, f32, f32)], time: f32) -> Vec3 {
    let mut col = Vec3::zeros();
    for &(cx, cy, r) in halos {
        let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / r.max(1.0);
        if d > 1.0 {
            let ang = (y - cy).atan2(x - cx);
            let rays =
                0.85 + 0.15 * (ang * 9.0 + time * 0.6).sin() * (ang * 5.0 - time * 0.4).cos();
            let glow = (-(d - 1.0) * 3.2).exp() * rays;
            col += hex(0xff8a2a) * glow * 0.9 + hex(0xffe2a0) * (-(d - 1.0) * 9.0).exp() * 0.6;
        }
    }
    col
}

fn unpack(c: u32) -> Vec3 {
    Vec3::new(
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    )
}

/// Suma un color a un píxel con prueba de profundidad contra la escena.
fn plot(fb: &mut Framebuffer, x: i32, y: i32, z: f32, col: Vec3) {
    if x < 0 || y < 0 || x >= fb.width as i32 || y >= fb.height as i32 {
        return;
    }
    let i = y as usize * fb.width + x as usize;
    if z >= fb.fragments[i].depth {
        return; // oculto detrás de un planeta
    }
    fb.color[i] = to_u32(unpack(fb.color[i]) + col);
}

/// Línea 3D: se proyectan los extremos y se recorre con DDA interpolando la profundidad.
fn line3d(fb: &mut Framebuffer, a: Vec3, b: Vec3, col: Vec3, u: &Uniforms) {
    let (Some(pa), Some(pb)) = (project(a, u), project(b, u)) else {
        return;
    };
    let steps = (pb - pa).xy().abs().max().ceil().min(4000.0) as i32;
    for s in 0..steps.max(1) {
        let t = s as f32 / steps.max(1) as f32;
        let p = pa + (pb - pa) * t;
        plot(fb, p.x as i32, p.y as i32, p.z, col);
    }
}

/// Dibuja las órbitas de los planetas (y de la luna) como círculos sobre la eclíptica.
fn draw_orbits(fb: &mut Framebuffer, u: &Uniforms) {
    const SEGMENTS: usize = 240;
    for b in BODIES.iter() {
        if b.orbit <= 0.0 {
            continue;
        }
        let (center, tilt) = match b.parent {
            Some(p) => (crate::scene::position(p, u.time), 0.12),
            None => (Vec3::zeros(), 0.0),
        };
        let point = |k: usize| {
            let a = k as f32 / SEGMENTS as f32 * TAU;
            center + Vec3::new(a.cos(), a.sin() * tilt, a.sin()) * b.orbit
        };
        let col = hex(b.color) * 0.3;
        for k in 0..SEGMENTS {
            line3d(fb, point(k), point(k + 1), col, u);
        }
    }
}

/// Efecto de warp: estelas radiales que salen del centro, tinte azul y viñeta.
fn warp_effect(fb: &mut Framebuffer, u: &Uniforms) {
    let e = u.warp.clamp(0.0, 1.0);
    let (wi, w, h) = (fb.width, fb.width as f32, fb.height as f32);
    let t = u.time;
    fb.color.par_iter_mut().enumerate().for_each(|(i, out)| {
        let x = (i % wi) as f32 + 0.5;
        let y = (i / wi) as f32 + 0.5;
        let dx = (x - w * 0.5) / (h * 0.5);
        let dy = (y - h * 0.5) / (h * 0.5);
        let r = (dx * dx + dy * dy).sqrt();
        let ang = dy.atan2(dx);
        // Cada "rayo" angular tiene su propia velocidad y fase.
        let a = (ang / TAU + 0.5) * 520.0;
        let bin = a.floor();
        let hb = hash2(bin, 7.0);
        // Estela delgada: solo el centro de cada sector angular.
        let thin = 1.0 - smoothstep(0.08, 0.3, (a - bin - 0.5).abs());
        let phase = (r * (0.6 + hb) - t * (2.5 + 3.0 * hb) + hb * 10.0).rem_euclid(1.0);
        let streak = thin * if hb > 0.7 {
            smoothstep(0.0, 0.03, phase) * (1.0 - smoothstep(0.03, 0.03 + 0.5 * e, phase))
        } else {
            0.0
        };
        let fade = smoothstep(0.12, 0.9, r);
        let base = unpack(*out);
        let tint = mix(base, base.component_mul(&Vec3::new(0.75, 0.9, 1.35)), e);
        let vignette = 1.0 - e * 0.45 * smoothstep(0.6, 1.6, r);
        let col = tint * vignette + hex(0xbfe0ff) * streak * fade * e;
        *out = to_u32(col);
    });
}
