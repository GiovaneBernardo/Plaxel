#[allow(dead_code)]
pub struct TransformComponent {
    pub position: crate::math::Vec3,
    pub rotation: crate::math::Quat,
    pub scale: crate::math::Vec3,
    pub velocity: crate::math::Vec3,
}

#[allow(dead_code)]
pub struct CameraComponent {
    pub speed: f32,
    pub fov: f32,
    pub far_plane: f32,
    pub near_plane: f32,
}
