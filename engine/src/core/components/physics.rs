#[derive(Clone, Copy, plaxel_reflect::Reflect)]
pub enum BodyKind {
    Dynamic,
    Fixed,
    Kinematic,
}

#[derive(Clone, plaxel_reflect::Reflect)]
pub enum ColliderShape {
    Sphere {
        radius: f32,
    },
    Cuboid {
        half_extents: crate::math::Vec3,
    },
    Trimesh {
        vertices: Vec<crate::math::Vec3>,
        indices: Vec<[u32; 3]>,
    },
}

#[derive(plaxel_reflect::Reflect)]
pub struct RigidBodyComponent {
    pub kind: BodyKind,
    pub mass: f32,
    pub velocity: crate::math::Vec3,
}

#[derive(plaxel_reflect::Reflect)]
pub struct ColliderComponent {
    pub shape: ColliderShape,
    pub restitution: f32,
    pub friction: f32,
}

#[derive(Clone, plaxel_reflect::Reflect)]
#[reflect(opaque)]
pub struct RapierRigidBodyHandle(pub rapier3d::dynamics::RigidBodyHandle);
#[derive(Clone, plaxel_reflect::Reflect)]
#[reflect(opaque)]
pub struct RapierColliderHandle(pub rapier3d::geometry::ColliderHandle);
