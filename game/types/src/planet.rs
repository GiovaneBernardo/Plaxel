pub struct Planet {
    pub id: u64,
    pub name: String,
    pub mesh: PlanetMesh,
}

pub struct PlanetMesh {
    pub positions: Vec<cgmath::Point3<f32>>,
}
