//! Cámara 3D. Tiene tres modos:
//!   - Persecución: detrás y arriba de la nave, la sigue en las tres dimensiones.
//!   - Cinemática: gira lentamente alrededor de la nave.
//!   - Vista general: desde arriba del plano eclíptico, mirando todo el sistema.
//! La cámara tampoco puede entrar en los planetas.

use crate::scene::{self, BODIES};
use crate::ship::Ship;
use nalgebra_glm::{self as glm, Mat4, Vec3};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CameraMode {
    Chase,
    Cinematic,
    Overview,
}

impl CameraMode {
    pub fn next(self) -> Self {
        match self {
            CameraMode::Chase => CameraMode::Cinematic,
            CameraMode::Cinematic => CameraMode::Overview,
            CameraMode::Overview => CameraMode::Chase,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CameraMode::Chase => "persecución",
            CameraMode::Cinematic => "cinemática",
            CameraMode::Overview => "vista general",
        }
    }
}

pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub mode: CameraMode,
    /// Campo de visión vertical en radianes (se abre durante el warp).
    pub fov: f32,
    /// Desplazamiento suavizado respecto a la nave (modo persecución).
    offset: Vec3,
    /// Ángulo del modo cinemático.
    orbit_angle: f32,
    /// Ángulo de la vista general (se puede girar).
    pub overview_angle: f32,
    pub overview_height: f32,
}

pub const BASE_FOV: f32 = 55.0;

impl Camera {
    pub fn new(ship: &Ship) -> Self {
        let mut c = Camera {
            eye: Vec3::zeros(),
            target: Vec3::zeros(),
            mode: CameraMode::Chase,
            fov: BASE_FOV.to_radians(),
            offset: Vec3::zeros(),
            orbit_angle: 0.0,
            overview_angle: 0.6,
            overview_height: 62.0,
        };
        c.offset = c.chase_offset(ship);
        c.update(ship, 0.0, 0.0);
        c
    }

    fn chase_offset(&self, ship: &Ship) -> Vec3 {
        let fwd = ship.forward();
        // Se aleja un poco más con la velocidad para dar sensación de rapidez.
        let back = 2.1 + ship.speed() * 0.035;
        -fwd * back + Vec3::new(0.0, 0.7, 0.0)
    }

    pub fn update(&mut self, ship: &Ship, dt: f32, t: f32) {
        let k = 1.0 - (-6.0 * dt).exp();
        let warp = ship.warp_intensity();
        self.fov = (BASE_FOV + 32.0 * warp).to_radians();

        match self.mode {
            CameraMode::Chase => {
                // Se suaviza el desplazamiento relativo (no la posición absoluta):
                // la cámara nunca se queda atrás aunque la nave vaya muy rápido.
                let desired = self.chase_offset(ship);
                self.offset += (desired - self.offset) * if dt == 0.0 { 1.0 } else { k };
                self.eye = ship.pos + self.offset;
                self.target = ship.pos + ship.forward() * 2.0;
            }
            CameraMode::Cinematic => {
                self.orbit_angle += dt * 0.35;
                let a = self.orbit_angle;
                self.eye = ship.pos + Vec3::new(a.cos() * 3.2, 1.1 + 0.4 * (a * 0.7).sin(), a.sin() * 3.2);
                self.target = ship.pos;
            }
            CameraMode::Overview => {
                let a = self.overview_angle;
                let h = self.overview_height;
                self.eye = Vec3::new(a.sin() * h * 0.85, h, a.cos() * h * 0.85);
                self.target = Vec3::zeros();
            }
        }

        // La cámara no puede atravesar los cuerpos.
        for (i, b) in BODIES.iter().enumerate() {
            let c = scene::position(i, t);
            let min = b.radius * 1.04 + 0.15;
            let d = self.eye - c;
            if d.norm() < min {
                self.eye = c + d.normalize() * min;
            }
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        glm::look_at(&self.eye, &self.target, &Vec3::y())
    }

    /// Base ortonormal de la cámara: (adelante, derecha, arriba).
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let f = (self.target - self.eye).normalize();
        let r = f.cross(&Vec3::y()).normalize();
        let u = r.cross(&f);
        (f, r, u)
    }
}
