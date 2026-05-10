use cgmath::vec3;
use engine::{
    core::{
        components::{
            core::TransformComponent,
            physics::{RigidBody, SphereCollider},
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
                SphereCollider {
                    radius: params.radius,
                },
            );
            world.insert(
                entity,
                RigidBody {
                    mass: params.mass,
                    velocity: cgmath::vec3(0.0, 0.0, 0.0),
                },
            );

            let Some(mut physics) = world.get_resource_mut::<Physics>() else {
                return;
            };

            physics.add_cuboid_collider(100.0, -0.5, 100.0);

            let ball_rigid_body_handle = physics.add_rigid_body_dynamic(&vec3(0.0, 10.0, 0.0));
            physics.add_sphere_collider(params.radius, Some(ball_rigid_body_handle));
        });
    }
}
