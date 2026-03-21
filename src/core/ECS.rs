use super::components;

#[derive(Copy, Clone)]
pub struct Entity {
    id: u64,
}

impl Entity {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_valid(&self) -> bool {
        self.id != 0
    }

    pub fn invalidate(&mut self) {
        self.id = 0;
    }
}

pub struct Scene {
    entities: Vec<Entity>,
    transform_components: Vec<components::core::TransformComponent>,
    mesh_renderers: Vec<components::renderer::MeshRenderer>,
    camera_components: Vec<components::core::CameraComponent>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            transform_components: Vec::new(),
            mesh_renderers: Vec::new(),
            camera_components: Vec::new(),
        }
    }

    pub fn create_entity(&mut self) -> Entity {
        let id = self.entities.len() as u64 + 1;
        let entity = Entity::new(id);
        self.entities.push(entity);
        entity
    }

    pub fn add_transform_component(
        &mut self,
        entity: &Entity,
        component: components::core::TransformComponent,
    ) {
        self.transform_components.push(component);
    }

    pub fn add_mesh_renderer(
        &mut self,
        entity: &Entity,
        mesh_renderer: components::renderer::MeshRenderer,
    ) {
        self.mesh_renderers.push(mesh_renderer);
    }

    pub fn get_instances(&self) -> Vec<Instance> {
        let mut instances: Vec<Instance> = Vec::new();
        for (transform, mesh_renderer) in self
            .transform_components
            .iter()
            .zip(self.mesh_renderers.iter())
        {
            // Here you would typically set up the rendering pipeline and draw the mesh using the transform.
            // This is a placeholder to indicate where rendering logic would go.
            instances.push(Instance {
                position,
                rotation,
                scale: 0.01,
            });
        }
        instances
    }
}
