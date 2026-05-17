#[derive(Clone, Copy)]
pub enum BodyKind {
    Dynamic,
    Fixed,
    Kinematic,
}

#[derive(Clone, Copy)]
pub enum ColliderShape {
    Sphere { radius: f32 },
    Cuboid { half_extents: cgmath::Vector3<f32> },
}

pub struct RigidBodyComponent {
    pub kind: BodyKind,
    pub mass: f32,
    pub velocity: cgmath::Vector3<f32>,
}

pub struct ColliderComponent {
    pub shape: ColliderShape,
    pub restitution: f32,
    pub friction: f32,
}

pub struct RapierRigidBodyHandle(pub rapier3d::dynamics::RigidBodyHandle);
pub struct RapierColliderHandle(pub rapier3d::geometry::ColliderHandle);
