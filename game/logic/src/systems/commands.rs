use cgmath::vec3;
use engine::{
    core::{
        components::{
            core::TransformComponent,
            physics::{BodyKind, ColliderComponent, ColliderShape, RigidBodyComponent},
        },
        physics::physics::Physics,
    },
    ecs::{commands::Commands, entity::Entity, world::World},
};
pub trait GameCommandsExt {
    fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams);
}
pub struct PhysicalSphereParams {
    pub position: cgmath::Vector3<f32>,
    pub radius: f32,
    pub mass: f32,
}

impl GameCommandsExt for Commands {
    fn spawn_physical_sphere(&mut self, params: PhysicalSphereParams) {
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
