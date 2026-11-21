//! Mallas del proyecto:
//!   - Icoesferas generadas por subdivisión (varios niveles de detalle).
//!   - Modelos OBJ propios (la nave), con normales planas por cara y una
//!     "parte" por objeto `o` del archivo para elegir el material.

use crate::vertex::Vertex;
use nalgebra_glm::Vec3;
use std::collections::HashMap;

/// Malla no indexada: cada 3 vértices forman un triángulo.
pub struct Mesh {
    pub vertices: Vec<Vertex>,
}

/// Radio de las esferas generadas (las instancias escalan a partir de aquí).
pub const SPHERE_RADIUS: f32 = 1.0;

/// Icoesfera: se parte de un icosaedro y cada triángulo se divide en 4,
/// proyectando los vértices nuevos sobre la esfera. Nivel 0 = 20 triángulos,
/// nivel n = 20 * 4^n triángulos.
pub fn icosphere(level: u32) -> Mesh {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut pos: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();

    let mut faces: Vec<[usize; 3]> = vec![
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
        [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
        [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];

    for _ in 0..level {
        // Cache de puntos medios para no duplicar vértices en aristas compartidas.
        let mut mid: HashMap<(usize, usize), usize> = HashMap::new();
        let mut midpoint = |a: usize, b: usize, pos: &mut Vec<Vec3>| -> usize {
            let key = if a < b { (a, b) } else { (b, a) };
            *mid.entry(key).or_insert_with(|| {
                pos.push(((pos[a] + pos[b]) * 0.5).normalize());
                pos.len() - 1
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = midpoint(a, b, &mut pos);
            let bc = midpoint(b, c, &mut pos);
            let ca = midpoint(c, a, &mut pos);
            next.push([a, ab, ca]);
            next.push([b, bc, ab]);
            next.push([c, ca, bc]);
            next.push([ab, bc, ca]);
        }
        faces = next;
    }

    let vertices = faces
        .iter()
        .flat_map(|f| f.iter().map(|&i| pos[i]))
        .map(|p| Vertex {
            position: p * SPHERE_RADIUS,
            normal: p,
            part: 0,
        })
        .collect();
    Mesh { vertices }
}

/// Carga un OBJ. Cada objeto (`o nombre`) se vuelve una parte; `part_of`
/// traduce el nombre del objeto a un índice de material. Si el archivo no trae
/// normales se calculan normales planas orientadas hacia afuera del objeto.
pub fn load_obj(path: &str, part_of: impl Fn(&str) -> u8) -> Result<Mesh, tobj::LoadError> {
    let (models, _) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            single_index: true,
            triangulate: true,
            ..Default::default()
        },
    )?;

    let mut vertices = Vec::new();
    for model in models {
        let mesh = &model.mesh;
        let part = part_of(&model.name);
        let p = |i: u32| {
            let i = i as usize;
            Vec3::new(
                mesh.positions[3 * i],
                mesh.positions[3 * i + 1],
                mesh.positions[3 * i + 2],
            )
        };
        // Centro del objeto: sirve para orientar las normales hacia afuera
        // (todas las piezas de la nave son convexas).
        let n_pos = mesh.positions.len() / 3;
        let center = (0..n_pos as u32).map(p).sum::<Vec3>() / n_pos.max(1) as f32;

        for tri in mesh.indices.chunks_exact(3) {
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let mut n = (b - a).cross(&(c - a));
            if n.norm() < 1e-9 {
                continue;
            }
            n = n.normalize();
            let (b, c) = if n.dot(&((a + b + c) / 3.0 - center)) < 0.0 {
                n = -n;
                (c, b) // invertir el orden para mantener la orientación consistente
            } else {
                (b, c)
            };
            for position in [a, b, c] {
                vertices.push(Vertex {
                    position,
                    normal: n,
                    part,
                });
            }
        }
    }
    Ok(Mesh { vertices })
}
