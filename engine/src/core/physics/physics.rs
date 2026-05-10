use cgmath::Vector3;
use rapier3d::{
    na::{ArrayStorage, Const},
    prelude::*,
};

pub struct Physics {
    pub rigid_body_set: RigidBodySet,
    pub collider_set: ColliderSet,
    pub gravity: Vector<Real>,
    pub integration_parameters: IntegrationParameters,
    pub physics_pipeline: PhysicsPipeline,
    pub island_manager: IslandManager,
    pub broad_phase: DefaultBroadPhase,
    pub narrow_phase: NarrowPhase,
    pub impulse_joint_set: ImpulseJointSet,
    pub multibody_joint_set: MultibodyJointSet,
    pub ccd_solver: CCDSolver,
    pub physics_hooks: (),
    pub event_handler: (),
    pub ball_body_handle: Option<RigidBodyHandle>,
}

impl Physics {
    pub fn new() -> Self {
        Self {
            rigid_body_set: RigidBodySet::new(),
            collider_set: ColliderSet::new(),
            gravity: vector![0.0, -9.81, 0.0],
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
        let gravity = vector!(0.0, -9.81, 0.0);
    }

    pub fn step(&mut self) {
        self.physics_pipeline.step(
            &self.gravity,
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

        if self.ball_body_handle != None {
            let ball_body = &self.rigid_body_set[self.ball_body_handle.unwrap()];
            println!("Ball altitude: {}", ball_body.translation().y);
        }
    }

    pub fn terminate() {}

    pub fn add_cuboid_collider(&mut self, pos_x: f32, pos_y: f32, pos_z: f32) {
        self.collider_set
            .insert(ColliderBuilder::cuboid(pos_x, pos_y, pos_z).build());
    }

    pub fn add_rigid_body_dynamic(
        &mut self,
        translation: &Vector3<f32>,
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

    /*/
               physics.add_cuboid_collider(100.0, -0.5, 100.0);

           let ball_rigid_body_handle = physics.add_rigid_body_dynamic();
           physics.add_sphere_collider(params.radius, ball_rigid_body_handle);
    */
}
