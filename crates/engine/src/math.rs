//! glam re-exports, `Transform`, and `Camera`. Owned by GE-003.
//!
//! Stub: filled in by its ticket. Do not add unrelated code here.
//!
//! GE-005 needs `Transform` for baked glTF node transforms before GE-003
//! lands, so the struct (fields, `IDENTITY`, `Default`) is defined here
//! exactly as GE-003 designs it. GE-003 replaces this file wholesale.

use glam::{Quat, Vec3};

/// Translation, rotation, and scale of an object (applied scale first,
/// then rotation, then translation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Position.
    pub translation: Vec3,
    /// Orientation.
    pub rotation: Quat,
    /// Per-axis scale.
    pub scale: Vec3,
}

impl Transform {
    /// No translation, no rotation, unit scale.
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
