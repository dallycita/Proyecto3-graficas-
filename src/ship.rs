//! La nave: física simple (aceleración, frenado y giro progresivos), marco de
//! referencia del planeta cercano, colisiones con los cuerpos y warp animado.

use crate::scene::{self, Instance, MeshKind, ShaderKind, BODIES};
use crate::shaders::common::smoothstep;
use nalgebra_glm::{self as glm, Mat4, Vec3};
use std::f32::consts::PI;

/// Escala del modelo de la nave en el mundo.
pub const SHIP_SCALE: f32 = 0.3;
/// Radio de colisión de la nave.
pub const SHIP_RADIUS: f32 = 0.45;
/// Duración del warp (segundos).
pub const WARP_TIME: f32 = 2.4;

const ACCEL: f32 = 6.0;
const BOOST_ACCEL: f32 = 22.0;
const MAX_SPEED: f32 = 9.0;
const BOOST_SPEED: f32 = 32.0;
const TURN_RATE: f32 = 1.5;
const PITCH_RATE: f32 = 1.1;
const LIFT_ACCEL: f32 = 4.0;

/// Intención del piloto para un cuadro (viene del teclado o del recorrido automático).
#[derive(Default, Clone, Copy)]
pub struct Input {
    /// -1..1: retroceso / avance.
    pub thrust: f32,
    /// -1..1: girar a la izquierda / derecha.
    pub turn: f32,
    /// -1..1: bajar / subir la nariz.
    pub pitch: f32,
    /// -1..1: descender / ascender (vertical del mundo).
    pub lift: f32,
    pub boost: bool,
    pub brake: bool,
    /// Iniciar un warp hacia este cuerpo.
    pub warp_to: Option<usize>,
}

pub struct Warp {
    pub target: usize,
    pub t: f32,
    from: Vec3,
    from_yaw: f32,
    from_pitch: f32,
}

pub struct Ship {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    yaw_rate: f32,
    pitch_rate: f32,
    /// Alabeo visual al girar.
    pub roll: f32,
    /// Potencia actual de los motores (0..1) para el brillo de las toberas.
    pub throttle: f32,
    pub warp: Option<Warp>,
    /// Cuerpo cuyo marco de referencia sigue la nave (si está cerca de uno).
    pub anchor: Option<usize>,
    /// Hubo colisión en este cuadro.
    pub bumped: bool,
}

/// Punto de llegada de un warp: frente al lado iluminado del cuerpo, un poco arriba.
pub fn arrival_point(i: usize, t: f32) -> Vec3 {
    let c = scene::position(i, t);
    let r = BODIES[i].radius;
    let dist = if BODIES[i].rings { r * 4.2 + 2.0 } else { r * 3.2 + 1.6 };
    let to_sun = if c.norm() > 1e-3 {
        (-c).normalize()
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    // Girar 35° para ver el terminador (día / noche) y no quedar alineados con el sol.
    let dir = crate::shaders::common::rot_y(to_sun, 0.6);
    c + (dir + Vec3::new(0.0, 0.28, 0.0)).normalize() * dist
}

fn yaw_pitch_towards(d: Vec3) -> (f32, f32) {
    let d = d.normalize();
    (d.x.atan2(d.z), d.y.clamp(-1.0, 1.0).asin())
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut diff = (b - a) % (2.0 * PI);
    if diff > PI {
        diff -= 2.0 * PI;
    } else if diff < -PI {
        diff += 2.0 * PI;
    }
    a + diff * t
}

impl Ship {
    pub fn new(t: f32) -> Self {
        let p = arrival_point(2, t);
        let (yaw, pitch) = yaw_pitch_towards(scene::position(2, t) - p);
        Ship {
            pos: p + (p - scene::position(2, t)).normalize() * 6.0,
            vel: Vec3::zeros(),
            yaw,
            pitch,
            yaw_rate: 0.0,
            pitch_rate: 0.0,
            roll: 0.0,
            throttle: 0.0,
            warp: None,
            anchor: None,
            bumped: false,
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// Intensidad del efecto visual del warp (sube y baja durante el viaje).
    pub fn warp_intensity(&self) -> f32 {
        match &self.warp {
            Some(w) => (w.t * PI).sin().powf(0.6),
            None => 0.0,
        }
    }

    pub fn speed(&self) -> f32 {
        self.vel.norm()
    }

    /// Avanza la simulación `dt` segundos. `t0` y `t1` son el tiempo del
    /// sistema antes y después del paso (los planetas se mueven entre ambos).
    pub fn update(&mut self, input: &Input, dt: f32, t0: f32, t1: f32) {
        self.bumped = false;

        if let Some(target) = input.warp_to {
            if self.warp.is_none() {
                self.warp = Some(Warp {
                    target,
                    t: 0.0,
                    from: self.pos,
                    from_yaw: self.yaw,
                    from_pitch: self.pitch,
                });
            }
        }

        if let Some(w) = &mut self.warp {
            // Warp: interpolación suave (ease in-out) hacia el punto de llegada,
            // que se recalcula cada cuadro porque el planeta sigue orbitando.
            w.t = (w.t + dt / WARP_TIME).min(1.0);
            let e = smoothstep(0.0, 1.0, w.t);
            let dest = arrival_point(w.target, t1);
            self.pos = w.from + (dest - w.from) * e;
            // Arco sobre la ecliptica: el trayecto se eleva en el medio para no
            // atravesar el sol (que esta en el plano y=0) ni cruzar otros cuerpos
            // en linea recta. La elevacion es 0 al inicio y al final del warp.
            let lift = (w.t * PI).sin() * ((dest - w.from).norm() * 0.17).min(14.0);
            self.pos.y += lift;
            let (ty, tp) = yaw_pitch_towards(scene::position(w.target, t1) - dest);
            let turn = smoothstep(0.0, 0.7, w.t);
            self.yaw = lerp_angle(w.from_yaw, ty, turn);
            self.pitch = w.from_pitch + (tp - w.from_pitch) * turn;
            self.vel = Vec3::zeros();
            self.yaw_rate = 0.0;
            self.pitch_rate = 0.0;
            self.roll *= 0.9;
            self.throttle = 1.0;
            if w.t >= 1.0 {
                self.warp = None;
            }
            self.anchor = nearest_anchor(self.pos, t1);
            return;
        }

        // Giro progresivo: la velocidad angular se acerca a la deseada.
        let k = 1.0 - (-5.0 * dt).exp();
        self.yaw_rate += (input.turn * TURN_RATE - self.yaw_rate) * k;
        self.pitch_rate += (input.pitch * PITCH_RATE - self.pitch_rate) * k;
        self.yaw += self.yaw_rate * dt;
        self.pitch = (self.pitch + self.pitch_rate * dt).clamp(-1.3, 1.3);
        self.roll += (-self.yaw_rate * 0.45 - self.roll) * k;

        // Aceleración, empuje vertical y arrastre.
        let fwd = self.forward();
        let accel = if input.boost { BOOST_ACCEL } else { ACCEL };
        self.vel += fwd * (input.thrust * accel * dt);
        self.vel += Vec3::new(0.0, input.lift * LIFT_ACCEL * dt, 0.0);
        let drag = if input.brake { 3.5 } else { 0.6 };
        self.vel *= (-drag * dt).exp();
        let max = if input.boost { BOOST_SPEED } else { MAX_SPEED };
        if self.vel.norm() > max {
            self.vel = self.vel.normalize() * max;
        }
        self.throttle += (input.thrust.abs().max(if input.boost { 1.0 } else { 0.0 })
            - self.throttle)
            * k;

        // Marco de referencia: cerca de un planeta la nave lo acompaña en su
        // órbita, así es fácil rodearlo sin que se "escape".
        self.anchor = nearest_anchor(self.pos, t0);
        if let Some(a) = self.anchor {
            self.pos += scene::position(a, t1) - scene::position(a, t0);
        }
        self.pos += self.vel * dt;

        self.collide(t1);
    }

    /// Colisión básica: la nave no puede entrar en ningún cuerpo. Se empuja
    /// hacia afuera y se elimina la componente de la velocidad hacia el centro.
    fn collide(&mut self, t: f32) {
        for (i, b) in BODIES.iter().enumerate() {
            let c = scene::position(i, t);
            let min = b.radius * 1.12 + SHIP_RADIUS;
            let d = self.pos - c;
            let dist = d.norm();
            if dist < min {
                let n = if dist > 1e-4 { d / dist } else { Vec3::y() };
                self.pos = c + n * min;
                let inward = self.vel.dot(&n);
                if inward < 0.0 {
                    self.vel -= n * inward * 1.3; // pequeño rebote
                }
                self.bumped = true;
            }
        }
    }

    pub fn instance(&self) -> Instance {
        let rot4 = glm::rotate(&Mat4::identity(), self.yaw, &Vec3::y())
            * glm::rotate(&Mat4::identity(), -self.pitch, &Vec3::x())
            * glm::rotate(&Mat4::identity(), self.roll, &Vec3::z());
        let model = glm::translate(&Mat4::identity(), &self.pos)
            * rot4
            * glm::scale(&Mat4::identity(), &Vec3::new(SHIP_SCALE, SHIP_SCALE, SHIP_SCALE));
        Instance {
            kind: ShaderKind::Ship,
            mesh: MeshKind::Ship,
            center: self.pos,
            radius: SHIP_SCALE * 1.8,
            rot: glm::mat4_to_mat3(&rot4),
            model,
        }
    }
}

/// Cuerpo (que no sea el sol) dentro de cuya zona de influencia está el punto.
/// Se elige el más pequeño primero para que la luna tenga prioridad sobre su planeta.
pub fn nearest_anchor(p: Vec3, t: f32) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, b) in BODIES.iter().enumerate() {
        if b.orbit <= 0.0 {
            continue;
        }
        let d = (p - scene::position(i, t)).norm();
        let reach = b.radius * 5.0 + 2.5;
        if d < reach {
            let score = d / reach;
            if best.map_or(true, |(_, s)| score < s) {
                best = Some((i, score));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Cuerpo más cercano (incluye el sol) y la distancia a su superficie.
pub fn nearest_body(p: Vec3, t: f32) -> (usize, f32) {
    let mut best = (0, f32::INFINITY);
    for (i, b) in BODIES.iter().enumerate() {
        let d = (p - scene::position(i, t)).norm() - b.radius;
        if d < best.1 {
            best = (i, d);
        }
    }
    best
}
