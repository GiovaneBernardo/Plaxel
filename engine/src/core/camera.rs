use cgmath::{
    EuclideanSpace, InnerSpace, Matrix3, Quaternion, Rotation, Rotation3, SquareMatrix, Vector3,
};
use winit::{event::MouseScrollDelta, keyboard::KeyCode};

pub struct Camera {
    pub position: cgmath::Point3<f32>,
    pub orientation: Quaternion<f32>,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
}

impl Camera {
    pub fn forward(&self) -> Vector3<f32> {
        self.orientation * Vector3::new(0.0, 0.0, -1.0)
    }

    pub fn right(&self) -> Vector3<f32> {
        self.orientation * Vector3::new(1.0, 0.0, 0.0)
    }

    pub fn up(&self) -> Vector3<f32> {
        self.orientation * Vector3::new(0.0, 1.0, 0.0)
    }

    /// Build an orientation that points the camera's local -Z along `forward`,
    /// keeping the camera's local +Y as close to `up_hint` as possible.
    pub fn look_at(forward: Vector3<f32>, up_hint: Vector3<f32>) -> Quaternion<f32> {
        let f = forward.normalize();
        let r = f.cross(up_hint).normalize();
        let u = r.cross(f);
        // Columns are world axes the local axes map to: local +X -> r, +Y -> u, +Z -> -f.
        let m = Matrix3::from_cols(r, u, -f);
        Quaternion::from(m)
    }

    pub fn build_view_projection_matrix(&self) -> cgmath::Matrix4<f32> {
        let view = cgmath::Matrix4::from(self.orientation.invert())
            * cgmath::Matrix4::from_translation(-self.position.to_vec());
        let proj = cgmath::perspective(cgmath::Deg(self.fovy), self.aspect, self.znear, self.zfar);
        OPENGL_TO_WGPU_MATRIX * proj * view
    }
}

// Maps cgmath/OpenGL clip-space depth [-1, 1] to wgpu reverse-Z [1, 0]:
// near plane -> 1, far plane -> 0. Pairs with depth_compare = Greater and
// depth clear = 0.0; on Depth32Float this gives much better far-plane precision.
#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::from_cols(
    cgmath::Vector4::new(1.0, 0.0,  0.0, 0.0),
    cgmath::Vector4::new(0.0, 1.0,  0.0, 0.0),
    cgmath::Vector4::new(0.0, 0.0, -0.5, 0.0),
    cgmath::Vector4::new(0.0, 0.0,  0.5, 1.0),
);

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub position: [f32; 3],
    // WGSL aligns vec3<f32> to 16 bytes, so we need padding to match the shader layout
    pub _padding: f32,
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_proj: cgmath::Matrix4::identity().into(),
            position: [0.0, 0.0, 0.0],
            _padding: 0.0,
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera) {
        self.view_proj = camera.build_view_projection_matrix().into();
        self.position = camera.position.into();
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
    is_shift_pressed: bool,
    is_roll_left_pressed: bool,
    is_roll_right_pressed: bool,
    pub is_right_click_pressed: bool,
    yaw_delta: f32,
    pitch_delta: f32,
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
            is_shift_pressed: false,
            is_roll_left_pressed: false,
            is_roll_right_pressed: false,
            yaw_delta: 0.0,
            pitch_delta: 0.0,
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
            KeyCode::ShiftLeft => {
                self.is_shift_pressed = is_pressed;
                true
            }
            KeyCode::KeyQ => {
                self.is_roll_left_pressed = is_pressed;
                true
            }
            KeyCode::KeyE => {
                self.is_roll_right_pressed = is_pressed;
                true
            }
            _ => false,
        }
    }

    pub fn handle_mouse_click(&mut self, is_pressed: bool) {
        self.is_right_click_pressed = is_pressed;
    }

    pub fn handle_mouse_scroll(&mut self, delta: MouseScrollDelta) {
        let mut scroll = match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 100.0,
        };

        scroll = scroll.clamp(-1.0, 1.0);

        let sensitivity: f32 = 0.2;
        let factor = (1.0f32 + sensitivity).powf(scroll);

        self.speed = (self.speed * factor).clamp(0.001, 1_000_000.0);
    }

    pub fn handle_mouse(&mut self, dx: f32, dy: f32) {
        self.yaw_delta += dx * 0.1;
        self.pitch_delta += dy * 0.1;
    }

    pub fn update_camera(&mut self, camera: &mut Camera) {
        if self.is_right_click_pressed {
            // Apply rotations in the camera's local frame so behavior is the
            // same regardless of where on the planet you are. Right-multiplying
            // by a quaternion built from a local axis (X = right, Y = up)
            // composes the rotation in the camera's own frame.
            let yaw = Quaternion::from_axis_angle(
                Vector3::unit_y(),
                cgmath::Rad(-self.yaw_delta.to_radians()),
            );
            let pitch = Quaternion::from_axis_angle(
                Vector3::unit_x(),
                cgmath::Rad(-self.pitch_delta.to_radians()),
            );
            camera.orientation = (camera.orientation * yaw * pitch).normalize();
        }

        self.yaw_delta = 0.0;
        self.pitch_delta = 0.0;

        // Roll (Q/E): rotate around local forward (-Z).
        let mut roll_amount = 0.0f32;
        if self.is_roll_left_pressed {
            roll_amount -= 1.0;
        }
        if self.is_roll_right_pressed {
            roll_amount += 1.0;
        }
        if roll_amount != 0.0 {
            let roll =
                Quaternion::from_axis_angle(-Vector3::unit_z(), cgmath::Rad(roll_amount * 0.02));
            camera.orientation = (camera.orientation * roll).normalize();
        }

        let mut final_speed = self.speed;
        if self.is_shift_pressed {
            let distance = camera.position.to_vec().magnitude();
            final_speed *= distance.sqrt() * 0.1;
        }

        let forward = camera.forward();
        let right = camera.right();
        let up = camera.up();

        if self.is_forward_pressed {
            camera.position += forward * final_speed;
        }
        if self.is_backward_pressed {
            camera.position -= forward * final_speed;
        }
        if self.is_right_pressed {
            camera.position += right * final_speed;
        }
        if self.is_left_pressed {
            camera.position -= right * final_speed;
        }
        if self.is_up_pressed {
            camera.position += up * final_speed;
        }
        if self.is_down_pressed {
            camera.position -= up * final_speed;
        }
    }
}
