use winit::keyboard::KeyCode;

pub struct Camera {
    pub position: cgmath::Point3<f32>,
    pub yaw: f32,
    pub pitch: f32,
    pub front: cgmath::Vector3<f32>,
    pub up: cgmath::Vector3<f32>,
    pub right: cgmath::Vector3<f32>,
    pub world_up: cgmath::Vector3<f32>,
    pub eye: cgmath::Point3<f32>,
    pub target: cgmath::Point3<f32>,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera {
    pub fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        let view = cgmath::Matrix4::look_at_rh(self.position, self.position + self.front, self.up);
        let proj = cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar);
        return OPENGL_TO_WGPU_MATRIX * proj * view;
    }
}

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::from_cols(
    cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 0.0),
    cgmath::Vector4::new(0.0, 0.0, 0.5, 1.0),
);

// We need this for Rust to store our data correctly for the shaders
#[repr(C)]
// This is so we can store this in a buffer
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    // We can't use cgmath with bytemuck directly, so we'll have
    // to convert the Matrix4 into a 4x4 f32 array
    pub view_proj: [[f32; 4]; 4],
}

impl CameraUniform {
    pub fn new() -> Self {
        use cgmath::SquareMatrix;
        Self {
            view_proj: cgmath::Matrix4::identity().into(),
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().into();
    }
}

pub struct CameraController {
    speed: f32,
    is_forward_pressed: bool,
    is_backward_pressed: bool,
    is_left_pressed: bool,
    is_right_pressed: bool,
    is_up_pressed: bool,
    is_down_pressed: bool,
    pub is_right_click_pressed: bool,
    yaw: f32,
    pitch: f32,
}

impl CameraController {
    pub fn new(speed: f32) -> Self {
        Self {
            speed,
            is_forward_pressed: false,
            is_backward_pressed: false,
            is_left_pressed: false,
            is_right_pressed: false,
            is_up_pressed: false,
            is_down_pressed: false,
            is_right_click_pressed: false,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    pub fn handle_key(&mut self, code: KeyCode, is_pressed: bool) -> bool {
        match code {
            KeyCode::KeyW | KeyCode::ArrowUp => {
                self.is_forward_pressed = is_pressed;
                true
            }
            KeyCode::KeyA | KeyCode::ArrowLeft => {
                self.is_left_pressed = is_pressed;
                true
            }
            KeyCode::KeyS | KeyCode::ArrowDown => {
                self.is_backward_pressed = is_pressed;
                true
            }
            KeyCode::KeyD | KeyCode::ArrowRight => {
                self.is_right_pressed = is_pressed;
                true
            }
            KeyCode::KeyC | KeyCode::PageDown => {
                self.is_down_pressed = is_pressed;
                true
            }
            KeyCode::Space | KeyCode::PageUp => {
                self.is_up_pressed = is_pressed;
                true
            }
            _ => false,
        }
    }

    pub fn handle_mouse_click(&mut self, is_pressed: bool) {
        self.is_right_click_pressed = is_pressed;
    }

    pub fn handle_mouse(&mut self, _x: f32, _y: f32) {
        self.yaw += _x as f32 * 0.1;
        self.pitch += _y as f32 * 0.1;
    }

    pub fn update_camera(&self, camera: &mut Camera) {
        use cgmath::InnerSpace;

        if self.is_right_click_pressed {
            // Update camera direction
            camera.yaw += self.yaw;
            camera.pitch += self.pitch;
            // Limit the pitch to prevent screen flip
            camera.pitch = camera.pitch.clamp(-89.0, 89.0);

            camera.front.x = self.yaw.to_radians().cos() * self.pitch.to_radians().cos();
            camera.front.y = -self.pitch.to_radians().sin();
            camera.front.z = self.yaw.to_radians().sin() * self.pitch.to_radians().cos();

            camera.right = camera.front.cross(camera.world_up).normalize();
            camera.up = camera.right.cross(camera.front).normalize();
        }

        // Forward Backward
        if self.is_forward_pressed {
            camera.position += camera.front * self.speed;
        }
        if self.is_backward_pressed {
            camera.position -= camera.front * self.speed;
        }

        // Right Left
        if self.is_right_pressed {
            camera.position += camera.right * self.speed;
        }
        if self.is_left_pressed {
            camera.position -= camera.right * self.speed;
        }

        // Up down
        if self.is_up_pressed {
            camera.position += camera.up.normalize() * self.speed;
        }
        if self.is_down_pressed {
            camera.position -= camera.up.normalize() * self.speed;
        }
    }
}
