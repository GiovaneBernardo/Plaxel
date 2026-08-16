//! Engine-owned math vocabulary backed by `glam`.
//!
//! Engine and game code should import math types through this module so the
//! chosen backend and the public vocabulary remain centralized.

pub use glam::{
    Affine2, Affine3A, BVec2, BVec3, BVec4, DAffine2, DAffine3, DMat2, DMat3, DMat4, DQuat, DVec2,
    DVec3, DVec4, IVec2, IVec3, IVec4, Mat2, Mat3, Mat4, Quat, UVec2, UVec3, UVec4, Vec2, Vec3,
    Vec3A, Vec4,
};

pub use glam::{dvec2, dvec3, dvec4, ivec2, ivec3, ivec4, uvec2, uvec3, uvec4, vec2, vec3, vec4};

pub mod prelude {
    pub use glam::{
        Affine2, Affine3A, BVec2, BVec3, BVec4, DAffine2, DAffine3, DMat2, DMat3, DMat4, DQuat,
        DVec2, DVec3, DVec4, IVec2, IVec3, IVec4, Mat2, Mat3, Mat4, Quat, UVec2, UVec3, UVec4,
        Vec2, Vec3, Vec3A, Vec4,
    };
    pub use glam::{
        dvec2, dvec3, dvec4, ivec2, ivec3, ivec4, uvec2, uvec3, uvec4, vec2, vec3, vec4,
    };
}
