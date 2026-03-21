#[allow(dead_code)]
pub struct TransformComponent {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
    pub scale: cgmath::Vector3<f32>,
    pub velocity: cgmath::Vector3<f32>,
}

#[allow(dead_code)]
pub struct CameraComponent {
    camera: crate::engine::camera::Camera,
    controller: crate::engine::camera::CameraController,
}
