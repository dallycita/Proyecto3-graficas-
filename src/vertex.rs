use nalgebra_glm::{Vec3, Vec4};

/// Vértice tal como viene del modelo.
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    /// Parte del modelo a la que pertenece (material). Las esferas usan 0.
    pub part: u8,
}

/// Vértice después del vertex shader, todavía en espacio de recorte (clip space).
/// Se recorta contra el plano cercano antes de dividir entre w.
#[derive(Clone, Copy, Debug)]
pub struct ClipVertex {
    pub clip: Vec4,
    /// Posición en el mundo.
    pub world: Vec3,
    /// Posición original en espacio del objeto.
    pub obj: Vec3,
    /// Normal en espacio del objeto.
    pub normal: Vec3,
}

impl ClipVertex {
    pub fn lerp(&self, o: &ClipVertex, t: f32) -> ClipVertex {
        ClipVertex {
            clip: self.clip + (o.clip - self.clip) * t,
            world: self.world + (o.world - self.world) * t,
            obj: self.obj + (o.obj - self.obj) * t,
            normal: self.normal + (o.normal - self.normal) * t,
        }
    }
}

/// Vértice listo para rasterizar (después de la división de perspectiva).
#[derive(Clone, Copy, Debug)]
pub struct ScreenVertex {
    /// x, y en píxeles; z = profundidad NDC.
    pub screen: Vec3,
    /// 1/w para interpolación con corrección de perspectiva.
    pub inv_w: f32,
    pub world: Vec3,
    pub obj: Vec3,
    pub normal: Vec3,
}
