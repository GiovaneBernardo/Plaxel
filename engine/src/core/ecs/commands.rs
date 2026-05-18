use crate::{
    core::components::{
        core::TransformComponent,
        physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
    },
    ecs::{component::Component, entity::Entity, world::World},
};

pub trait Command {
    fn apply(self: Box<Self>, world: &mut World);
}

impl<F> Command for F
where
    F: FnOnce(&mut World),
{
    fn apply(self: Box<Self>, world: &mut World) {
        self(world);
    }
}

pub struct Commands {
    queue: Vec<Box<dyn Command>>,
}

pub struct PhysicalSphereParams {
    pub position: cgmath::Vector3<f32>,
    pub radius: f32,
    pub mass: f32,
}

impl Commands {
    pub fn new() -> Self {
        Self { queue: Vec::new() }
    }

    pub fn push(&mut self, command: impl FnOnce(&mut World) + 'static) {
        self.queue.push(Box::new(command));
    }

    pub fn spawn(&mut self) {
        self.push(|world: &mut World| {
            world.spawn();
        });
    }

    pub fn insert<T: Component>(&mut self, entity: Entity, component: T) {
        self.push(move |world: &mut World| {
            world.insert(entity, component);
        });
    }

    pub fn apply(&mut self, world: &mut World) {
        for command in self.queue.drain(..) {
            command.apply(world);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams) {
        self.push(move |world: &mut World| {
            let entity = world.spawn();
            world.insert(
                entity,
                TransformComponent {
                    position: params.position,
                    rotation: cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                    scale: cgmath::vec3(params.radius, params.radius, params.radius),
                    velocity: cgmath::vec3(0.0, 0.0, 0.0),
                },
            );
            world.insert(
                entity,
                ColliderComponent {
                    shape: ColliderShape::Sphere {
                        radius: params.radius,
                    },
                    friction: 0.5,
                    restitution: 0.5,
                },
            );
            world.insert(
                entity,
                RigidBodyComponent {
                    kind: BodyKind::Dynamic,
                    mass: params.mass,
                    velocity: cgmath::vec3(0.0, 0.0, 0.0),
                },
            );
        });
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}
