use crate::math::Vec3;
use rapier3d::prelude::*;

use crate::{
    core::components::{
        self,
        core::TransformComponent,
        physics::{
            ColliderComponent, RapierColliderHandle, RapierRigidBodyHandle, RigidBodyComponent,
        },
    },
    ecs::{
        commands::Commands, plugin::Plugin, query::Query, resource::ResMut, schedule::CoreSchedule,
        system::SystemContext,
    },
};

pub struct PhysicsPlugin;

impl Plugin for PhysicsPlugin {
    fn build(&self, app: &mut crate::App) {
        app.insert_resource(Physics::new())
            .add_system(CoreSchedule::PostUpdate, step_physics);
    }
}

fn step_physics(mut physics: ResMut<Physics>) {
    crate::profile_scope!("physics.step");
    physics.step();
}

#[derive(plaxel_reflect::Reflect)]
#[reflect(from_reflect = false)]
pub struct Physics {
    #[reflect(ignore)]
    pub rigid_body_set: RigidBodySet,
    #[reflect(ignore)]
    pub collider_set: ColliderSet,
    pub gravity: Vec3,
    #[reflect(ignore)]
    pub integration_parameters: IntegrationParameters,
    #[reflect(ignore)]
    pub physics_pipeline: PhysicsPipeline,
    #[reflect(ignore)]
    pub island_manager: IslandManager,
    #[reflect(ignore)]
    pub broad_phase: DefaultBroadPhase,
    #[reflect(ignore)]
    pub narrow_phase: NarrowPhase,
    #[reflect(ignore)]
    pub impulse_joint_set: ImpulseJointSet,
    #[reflect(ignore)]
    pub multibody_joint_set: MultibodyJointSet,
    #[reflect(ignore)]
    pub ccd_solver: CCDSolver,
    #[reflect(ignore)]
    pub physics_hooks: (),
    #[reflect(ignore)]
    pub event_handler: (),
    #[reflect(ignore)]
    pub ball_body_handle: Option<RigidBodyHandle>,
}

impl Physics {
    pub fn new() -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: Vec3::new(0.0, -9.81, 0.0),
            integration_parameters: IntegrationParameters::default(),
            physics_pipeline: PhysicsPipeline::new(),
            island_manager: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            impulse_joint_set: ImpulseJointSet::new(),
            multibody_joint_set: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            physics_hooks: (),
            event_handler: (),
            ball_body_handle: None,
        }
    }

    pub fn init() {
        let _gravity = vector!(0.0, -9.81, 0.0);
    }

    pub fn step(&mut self) {
        crate::profile_counter!("physics.bodies", self.rigid_body_set.len() as f64);
        crate::profile_counter!("physics.colliders", self.collider_set.len() as f64);
        let gravity = vector![self.gravity.x, self.gravity.y, self.gravity.z];
        self.physics_pipeline.step(
            &gravity,
            &self.integration_parameters,
            &mut self.island_manager,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.rigid_body_set,
            &mut self.collider_set,
            &mut self.impulse_joint_set,
            &mut self.multibody_joint_set,
            &mut self.ccd_solver,
            &self.physics_hooks,
            &self.event_handler,
        );
    }

    pub fn terminate() {}

    pub fn add_cuboid_collider(&mut self, pos_x: f32, pos_y: f32, pos_z: f32) {
        self.collider_set
            .insert(ColliderBuilder::cuboid(pos_x, pos_y, pos_z).build());
    }

    pub fn add_rigid_body_dynamic(
        &mut self,
        translation: &Vec3,
    ) -> rapier3d::dynamics::RigidBodyHandle {
        let rigid_body = RigidBodyBuilder::dynamic()
            .translation(vector![translation.x, translation.y, translation.z])
            .build();
        self.ball_body_handle = Some(self.rigid_body_set.insert(rigid_body));
        self.ball_body_handle.unwrap()
    }

    pub fn add_sphere_collider(
        &mut self,
        radius: f32,
        rigid_body_handle: Option<rapier3d::dynamics::RigidBodyHandle>,
    ) {
        let collider = ColliderBuilder::ball(radius).restitution(0.7).build();
        if rigid_body_handle != None {
            self.collider_set.insert_with_parent(
                collider,
                rigid_body_handle.unwrap(),
                &mut self.rigid_body_set,
            );
        }
    }

    pub fn add_trimesh_collider(
        &mut self,
        vertices: Vec<Vec3>,
        indices: Vec<[u32; 3]>,
        restitution: f32,
        friction: f32,
    ) -> Option<ColliderHandle> {
        let vertices = vertices
            .into_iter()
            .map(|p| point![p.x, p.y, p.z])
            .collect();
        let builder = ColliderBuilder::trimesh(vertices, indices).ok()?;
        let collider = builder.restitution(restitution).friction(friction).build();
        Some(self.collider_set.insert(collider))
    }

    pub fn remove_collider(&mut self, handle: ColliderHandle) {
        self.collider_set.remove(
            handle,
            &mut self.island_manager,
            &mut self.rigid_body_set,
            true,
        );
    }

    pub fn create_missing_rapier_bodies_system(ctx: &mut SystemContext, commands: &mut Commands) {
        let world = &mut ctx.world;
        let Some(mut physics) = world.get_resource_mut::<Physics>() else {
            return;
        };
        let Physics {
            rigid_body_set,
            collider_set,
            ..
        } = &mut *physics;
        let mut query = Query::<(&mut ColliderComponent,)>::new(world);
        //println!(query.iter_size());
        query.for_each(|entity, (collider,)| {
            if world.get::<RapierColliderHandle>(entity).is_some() {
                return;
            }

            if world.get::<RapierRigidBodyHandle>(entity).is_some() {
                return;
            }

            let Some(transform) = world.get::<TransformComponent>(entity) else {
                return;
            };

            let rapier_collider = match collider.shape.clone() {
                components::physics::ColliderShape::Sphere { radius } => {
                    ColliderBuilder::ball(radius)
                }
                components::physics::ColliderShape::Cuboid { half_extents } => {
                    ColliderBuilder::cuboid(half_extents.x, half_extents.y, half_extents.z)
                }
                components::physics::ColliderShape::Trimesh { vertices, indices } => {
                    let vertices = vertices
                        .into_iter()
                        .map(|p| {
                            point![
                                p.x * transform.scale.x,
                                p.y * transform.scale.y,
                                p.z * transform.scale.z
                            ]
                        })
                        .collect();
                    let Ok(builder) = ColliderBuilder::trimesh(vertices, indices) else {
                        return;
                    };
                    builder
                }
            }
            .restitution(collider.restitution)
            .friction(collider.friction)
            .build();

            let rigid_body_component = world.get::<RigidBodyComponent>(entity);
            if let Some(rigid_body_component) = rigid_body_component {
                let mut builder = match rigid_body_component.kind {
                    components::physics::BodyKind::Dynamic => RigidBodyBuilder::dynamic(),
                    components::physics::BodyKind::Fixed => RigidBodyBuilder::fixed(),
                    components::physics::BodyKind::Kinematic => {
                        RigidBodyBuilder::kinematic_position_based()
                    }
                };

                builder = builder.translation(vector![
                    transform.position.x,
                    transform.position.y,
                    transform.position.z
                ]);

                if matches!(
                    rigid_body_component.kind,
                    components::physics::BodyKind::Dynamic
                ) {
                    builder = builder.additional_mass(rigid_body_component.mass);
                }

                let rapier_rigid_body_handle = rigid_body_set.insert(builder.build());
                let rapier_collider_handle = collider_set.insert_with_parent(
                    rapier_collider,
                    rapier_rigid_body_handle,
                    rigid_body_set,
                );

                commands.insert(entity, RapierRigidBodyHandle(rapier_rigid_body_handle));
                commands.insert(entity, RapierColliderHandle(rapier_collider_handle));
            } else {
                let mut rapier_collider = rapier_collider;
                rapier_collider.set_translation(vector![
                    transform.position.x,
                    transform.position.y,
                    transform.position.z
                ]);
                commands.insert(
                    entity,
                    RapierColliderHandle(collider_set.insert(rapier_collider)),
                );
            }
        });
    }
}

pub fn cook_trimesh_indices(vertices: &[crate::math::Vec3], indices: &[u32]) -> Vec<[u32; 3]> {
    let vertex_count = vertices.len() as u32;
    let mut triangles = Vec::with_capacity(indices.len() / 3);

    for triangle in indices.chunks_exact(3) {
        let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
        if a >= vertex_count || b >= vertex_count || c >= vertex_count {
            continue;
        }

        let pa = vertices[a as usize];
        let pb = vertices[b as usize];
        let pc = vertices[c as usize];
        if !pa.x.is_finite()
            || !pa.y.is_finite()
            || !pa.z.is_finite()
            || !pb.x.is_finite()
            || !pb.y.is_finite()
            || !pb.z.is_finite()
            || !pc.x.is_finite()
            || !pc.y.is_finite()
            || !pc.z.is_finite()
        {
            continue;
        }

        let ab = pb - pa;
        let ac = pc - pa;
        if ab.cross(ac).length_squared() <= f32::EPSILON {
            continue;
        }

        triangles.push([a, b, c]);
    }

    triangles
}
