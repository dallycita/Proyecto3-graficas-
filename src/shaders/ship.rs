//! NAVE "Colibrí" — materiales por pieza del modelo OBJ.
//!
//!   0 casco:    cerámica blanca con paneles (líneas procedurales) y franja roja.
//!   1 cabina:   vidrio oscuro con reflejo de Fresnel azulado.
//!   2 alas:     verde azulado oscuro con puntas naranjas.
//!   3 motores:  metal pavonado.
//!   4 toberas:  emisivas; el brillo depende de la potencia y parpadea con el tiempo.
//! Iluminación: Lambert + Blinn-Phong con la luz del sol, luz ambiental fría y
//! luz de borde (rim) para que la silueta se lea contra el espacio.

use super::common::*;
use super::Surface;
use crate::noise::*;
use crate::render::Uniforms;
use nalgebra_glm::Vec3;

pub fn shade(s: &Surface, u: &Uniforms, nz: &Noises) -> Vec3 {
    let n = s.n;
    let p = s.obj;
    let ndl = n.dot(&s.l).max(0.0);
    let rim = (1.0 - n.dot(&s.v).abs()).powf(3.0);

    let (albedo, shininess, spec_k) = match s.part {
        0 => {
            // Paneles: líneas finas a lo largo y a lo ancho del casco.
            let panel_z = ((p.z * 4.0).fract() - 0.5).abs();
            let panel_x = ((p.x * 5.0 + 0.5).fract() - 0.5).abs();
            let lines = smoothstep(0.47, 0.5, panel_z.max(panel_x));
            let mut c = mix(hex(0xe6e8ee), hex(0x9aa0ab), lines * 0.8);
            // Franja roja cerca de la punta.
            let stripe = smoothstep(0.04, 0.0, (p.z - 0.9).abs() - 0.08);
            c = mix(c, hex(0xd8343a), stripe);
            (c, 40.0, 0.5)
        }
        1 => {
            let fresnel = (1.0 - n.dot(&s.v).max(0.0)).powf(2.0);
            (mix(hex(0x0b1624), hex(0x5fc8ff), fresnel), 120.0, 1.2)
        }
        2 => {
            let tip = smoothstep(1.15, 1.3, p.x.abs());
            (mix(hex(0x1f4e5a), hex(0xff8a2a), tip), 30.0, 0.35)
        }
        3 => (hex(0x4a4f58), 60.0, 0.8),
        _ => {
            // Toberas: emisión que no depende del sol.
            let flicker = 0.85 + 0.15 * n3(&nz.simplex, Vec3::new(u.time * 12.0, p.x * 9.0, 0.0));
            let power = 0.25 + 0.75 * u.thrust;
            let core = mix(hex(0xff7a1a), hex(0x9fe8ff), u.thrust);
            return core * (1.4 * power * flicker) + hex(0xffffff) * u.warp * 0.6;
        }
    };

    let spec = blinn(n, s.l, s.v, shininess) * spec_k * if ndl > 0.0 { 1.0 } else { 0.0 };
    let ambient = hex(0x1c2433);
    albedo.component_mul(&(ambient + hex(0xfff1dd) * ndl)) + Vec3::repeat(spec) + hex(0x6fa8ff) * rim * 0.35
}
