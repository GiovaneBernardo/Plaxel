#[allow(dead_code)]
pub struct TransformComponent {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
    pub scale: cgmath::Vector3<f32>,
    pub velocity: cgmath::Vector3<f32>,
}

#[allow(dead_code)]
pub struct CameraComponent {
    pub speed: f32,
    pub fov: f32,
    pub far_plane: f32,
    pub near_plane: f32,
}
