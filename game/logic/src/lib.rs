use cgmath;
use engine::assets;
use engine::renderer::RenderNode;
use engine::{KeyCode, model::Mesh};
use game_types::planet::Planet;
use game_types::planet::PlanetMesh;
pub use game_types::render_graph;
use std::cmp;

#[unsafe(no_mangle)]
pub fn register_systems(state: &mut engine::State) {
    state
        .renderer
        .render_graph
        .nodes
        .push(render_graph::PlanetRendererNode::new(
            "planet".to_string(),
            vec![],
            vec![],
            vec![],
            vec![],
        ));
}

#[unsafe(no_mangle)]
pub fn render() {
    // libloading: load game_logic.dll, find "render", call it
}

#[unsafe(no_mangle)]
pub fn update(state: &mut engine::State) {
    for transform in &mut state.scene.transform_components {
        transform.scale = (0.01, 0.01, 0.01).into(); //(transform.velocity.x * 0.1);
        transform.position -= transform.velocity;
    }

    let mut planet = Planet::generate_planet();
    planet.load_mesh();
    if planet.mesh.positions.len() > 0 {
        let render_data = state
            .renderer
            .create_render_data(&state.device, planet.mesh.positions);

        state
            .renderer
            .render_graph
            .nodes
            .first_mut()
            .unwrap()
            .add_render_data(render_data);
    }
}

#[unsafe(no_mangle)]
pub fn handle_key_press(state: &mut engine::State, key_code: KeyCode, pressed: bool) {
    if key_code == KeyCode::KeyU && pressed {
        for i in 0..cmp::min(state.scene.transform_components.len(), 3) {
            state.scene.transform_components[i].position.y += 0.1;
        }
    }

    if key_code == KeyCode::KeyT && pressed {}
}

trait PlanetExt {
    fn generate_planet() -> Self;
    fn load_mesh(&mut self);
}

impl PlanetExt for Planet {
    fn generate_planet() -> Self {
        Planet {
            id: 0,
            name: String::new(),
            mesh: PlanetMesh::new(),
        }
    }

    fn load_mesh(&mut self) {
        self.mesh.positions = vec![
            cgmath::Point3::new(-0.5, -0.5, -0.5),
            cgmath::Point3::new(0.5, -0.5, -0.5),
            cgmath::Point3::new(0.5, 0.5, -0.5),
            cgmath::Point3::new(-0.5, 0.5, -0.5),
            cgmath::Point3::new(-0.5, -0.5, 0.5),
            cgmath::Point3::new(0.5, -0.5, 0.5),
            cgmath::Point3::new(0.5, 0.5, 0.5),
            cgmath::Point3::new(-0.5, 0.5, 0.5),
        ];
    }
}

trait PlanetMeshExt {
    fn new() -> Self;
}

impl PlanetMeshExt for PlanetMesh {
    fn new() -> Self {
        PlanetMesh {
            positions: Vec::new(),
        }
    }
}

impl RenderNode for PlanetRendererNode {
    fn new() -> Self {
        PlanetRendererNode {
            render_data: Vec::new(),
        }
    }

    fn add_render_data(&mut self, render_data: RenderData) {
        self.render_data.push(render_data);
    }
}
