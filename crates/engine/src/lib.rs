//! A small, readable 3D game engine.
//!
//! Module map (one subsystem per module; tickets map to modules so parallel
//! work rarely touches the same file):
//!
//! - [`app`]    — window, event loop, fixed-timestep game loop    (GE-001)
//! - [`gpu`]    — wgpu device/queue/surface context, offscreen targets (GE-001, GE-002)
//! - [`math`]   — glam re-exports, `Transform`, `Camera`          (GE-003)
//! - [`ecs`]    — hecs `World` wrapper, systems schedule, `Time`  (GE-004)
//! - [`asset`]  — glTF + texture loading into CPU-side structs    (GE-005)
//! - [`render`] — mesh upload, materials, shaders, instancing     (GE-006, GE-008)
//! - [`input`]  — keyboard/mouse state, camera controller         (GE-007)
//! - [`stats`]  — frame-time statistics                           (GE-009)
//!
//! Keep this file to module declarations and re-exports only.

pub mod app;
pub mod asset;
pub mod ecs;
pub mod gpu;
pub mod input;
pub mod math;
pub mod render;
pub mod stats;
