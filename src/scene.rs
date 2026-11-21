//! El sistema solar "Aurelia": definición de los cuerpos, sus órbitas sobre el
//! plano eclíptico (y = 0) y la construcción de las instancias de cada cuadro.
//!
//! Todos los cuerpos son la MISMA icoesfera; cada instancia solo cambia su
//! matriz de modelo (traslación orbital * inclinación * giro propio * escala)
//! y el fragment shader que la colorea.

use nalgebra_glm::{self as glm, Mat3, Mat4, Vec3};
use std::f32::consts::TAU;

pub use crate::mesh::SPHERE_RADIUS;
/// Escala horizontal de la matriz de modelo de los anillos (radio externo ≈ 2.4 radios del planeta).
pub const RING_SCALE: f32 = 2.5;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ShaderKind {
    Star,
    Rocky,
    Moon,
    GasGiant,
    Rings,
    Lava,
    Ice,
    Alien,
    Ship,
}

/// Qué malla dibuja la instancia.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MeshKind {
    Sphere,
    Ship,
}

#[derive(Clone, Copy)]
pub struct Instance {
    pub kind: ShaderKind,
    pub mesh: MeshKind,
    pub center: Vec3,
    /// Radio de la esfera envolvente (para descarte, colisiones y nivel de detalle).
    pub radius: f32,
    /// Rotación del objeto (inclinación * giro). Lleva vectores del mundo al objeto con su transpuesta.
    pub rot: Mat3,
    pub model: Mat4,
}

impl Instance {
    /// Instancia de un cuerpo esférico con inclinación axial y ángulo de giro propio.
    pub fn body(kind: ShaderKind, center: Vec3, radius: f32, tilt_deg: f32, spin: f32) -> Self {
        let tilt = glm::rotate(
            &Mat4::identity(),
            tilt_deg.to_radians(),
            &Vec3::new(0.0, 0.0, 1.0),
        );
        // Los anillos no giran con el planeta (su textura ya se mueve en el shader).
        let spin = if kind == ShaderKind::Rings { 0.0 } else { spin };
        let rot4 = tilt * glm::rotate(&Mat4::identity(), spin, &Vec3::new(0.0, 1.0, 0.0));
        let s = radius / SPHERE_RADIUS;
        // Los anillos son la MISMA esfera con una escala no uniforme: muy
        // delgada en Y y ancha en XZ (un disco).
        let scale = if kind == ShaderKind::Rings {
            Vec3::new(s * RING_SCALE, s * 0.002, s * RING_SCALE)
        } else {
            Vec3::new(s, s, s)
        };
        let model = glm::translate(&Mat4::identity(), &center)
            * rot4
            * glm::scale(&Mat4::identity(), &scale);
        let bound = if kind == ShaderKind::Rings {
            radius * RING_SCALE
        } else {
            radius
        };
        Instance {
            kind,
            mesh: MeshKind::Sphere,
            center,
            radius: bound,
            rot: glm::mat4_to_mat3(&rot4),
            model,
        }
    }
}

/// Parámetros físicos de un cuerpo del sistema.
pub struct Body {
    pub name: &'static str,
    pub kind: ShaderKind,
    pub radius: f32,
    /// Radio de la órbita (alrededor del sol o de `parent`).
    pub orbit: f32,
    /// Fase inicial de la órbita (radianes).
    pub phase: f32,
    /// Periodo de rotación propia en segundos (negativo = retrógrado).
    pub day: f32,
    pub tilt: f32,
    pub parent: Option<usize>,
    /// Tiene anillos.
    pub rings: bool,
    /// Color de su línea de órbita.
    pub color: u32,
}

/// Constante "gravitacional" del sistema: velocidad angular = K / r^1.5 (tercera ley de Kepler).
const KEPLER_K: f32 = 4.8;

pub const BODIES: [Body; 7] = [
    Body { name: "Helios", kind: ShaderKind::Star, radius: 3.0, orbit: 0.0, phase: 0.0, day: 60.0, tilt: 0.0, parent: None, rings: false, color: 0xffb040 },
    Body { name: "Ignis", kind: ShaderKind::Lava, radius: 0.6, orbit: 8.0, phase: 0.4, day: 14.0, tilt: 3.0, parent: None, rings: false, color: 0xff6a3a },
    Body { name: "Terra Nova", kind: ShaderKind::Rocky, radius: 1.0, orbit: 13.5, phase: 2.2, day: 20.0, tilt: 18.0, parent: None, rings: false, color: 0x5fb4ff },
    Body { name: "Selene", kind: ShaderKind::Moon, radius: 0.28, orbit: 2.2, phase: 0.0, day: 30.0, tilt: 0.0, parent: Some(2), rings: false, color: 0x707070 },
    Body { name: "Nyx", kind: ShaderKind::Alien, radius: 0.85, orbit: 19.0, phase: 4.1, day: -16.0, tilt: 10.0, parent: None, rings: false, color: 0x7dff9a },
    Body { name: "Zephyr", kind: ShaderKind::GasGiant, radius: 1.7, orbit: 27.0, phase: 1.0, day: 12.0, tilt: 22.0, parent: None, rings: true, color: 0xe8c48a },
    Body { name: "Glacies", kind: ShaderKind::Ice, radius: 0.95, orbit: 35.0, phase: 5.3, day: 26.0, tilt: 28.0, parent: None, rings: false, color: 0x9be8ff },
];

/// Velocidad angular orbital (rad/s) de un cuerpo.
pub fn orbit_speed(b: &Body) -> f32 {
    if b.orbit <= 0.0 {
        return 0.0;
    }
    if b.parent.is_some() {
        // Las lunas orbitan más rápido alrededor de su planeta.
        return TAU / 18.0;
    }
    KEPLER_K / b.orbit.powf(1.5)
}

/// Posición de un cuerpo en el tiempo `t`. Las órbitas son circulares y
/// están sobre el plano eclíptico (y = 0); la luna tiene una leve inclinación.
pub fn position(i: usize, t: f32) -> Vec3 {
    let b = &BODIES[i];
    let a = b.phase + orbit_speed(b) * t;
    let local = Vec3::new(a.cos() * b.orbit, 0.0, a.sin() * b.orbit);
    match b.parent {
        Some(p) => position(p, t) + Vec3::new(local.x, local.z * 0.12, local.z),
        None => local,
    }
}

/// Ángulo de giro propio en el tiempo `t`.
pub fn spin(i: usize, t: f32) -> f32 {
    TAU * t / BODIES[i].day
}


/// Instancias de todos los cuerpos en el tiempo `t` (sin la nave).
pub fn build(t: f32) -> Vec<Instance> {
    let mut out = Vec::with_capacity(BODIES.len() + 1);
    for (i, b) in BODIES.iter().enumerate() {
        let c = position(i, t);
        out.push(Instance::body(b.kind, c, b.radius, b.tilt, spin(i, t)));
        if b.rings {
            out.push(Instance::body(ShaderKind::Rings, c, b.radius, b.tilt, 0.0));
        }
    }
    out
}
