//! The editor's orbit camera: left-drag orbits around a target point,
//! right-drag pans the target, the wheel zooms.
//!
//! [`OrbitCamera`] holds only the orbit parameters; [`OrbitCamera::apply`]
//! writes the resulting placement into the engine's [`Camera`] resource.

use shin::math::{Camera, Quat, Transform, Vec2, Vec3};

/// Radians of yaw or pitch per point of mouse drag.
const ORBIT_SPEED: f32 = 0.008;
/// Target movement per point of drag, as a fraction of the distance, so
/// panning feels the same at any zoom level.
const PAN_SPEED: f32 = 0.0015;
/// Zoom rate per point of scroll: the distance is multiplied by
/// `exp(-scroll * ZOOM_SPEED)`.
const ZOOM_SPEED: f32 = 0.0015;
/// Pitch stays strictly inside ±90° so the view never flips over the pole.
const MAX_PITCH: f32 = 89f32.to_radians();
/// Closest the camera may get to its target.
const MIN_DISTANCE: f32 = 0.1;
/// Farthest the camera may get from its target.
const MAX_DISTANCE: f32 = 500.0;

/// A camera circling `target` at `distance`, described by yaw and pitch.
///
/// Yaw 0 and pitch 0 put the camera on the target's +Z side looking along
/// -Z; negative pitch lifts the camera so it looks down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// The point the camera looks at and orbits around.
    pub target: Vec3,
    /// Rotation about +Y, in radians.
    pub yaw: f32,
    /// Rotation about the camera's right axis, in radians, within ±89°.
    pub pitch: f32,
    /// Distance from the target.
    pub distance: f32,
}

impl Default for OrbitCamera {
    /// Above and in front of the origin, looking down at a unit cube
    /// resting on y = 0.
    fn default() -> Self {
        Self {
            target: Vec3::new(0.0, 0.5, 0.0),
            yaw: 0.0,
            pitch: -25f32.to_radians(),
            distance: 6.0,
        }
    }
}

impl OrbitCamera {
    /// The camera's orientation.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }

    /// Where the camera is and which way it faces; its forward points at
    /// [`Self::target`].
    pub fn transform(&self) -> Transform {
        let rotation = self.rotation();
        let translation = self.target + rotation * Vec3::Z * self.distance;
        Transform::from_trs(translation, rotation, Vec3::ONE)
    }

    /// Orbits by a drag of `delta` points (x right, y down): dragging
    /// right swings the camera left around the target, dragging down
    /// raises it.
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * ORBIT_SPEED;
        self.pitch = (self.pitch - delta.y * ORBIT_SPEED).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Pans by a drag of `delta` points (x right, y down): the scene follows
    /// the pointer, so the target moves the opposite way in view space.
    pub fn pan(&mut self, delta: Vec2) {
        let t = self.transform();
        let scale = self.distance * PAN_SPEED;
        self.target += (-t.right() * delta.x + t.up() * delta.y) * scale;
    }

    /// Zooms by `scroll` points of wheel movement; positive (wheel up)
    /// moves closer.
    pub fn zoom(&mut self, scroll: f32) {
        self.distance =
            (self.distance * (-scroll * ZOOM_SPEED).exp()).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Moves `camera` to this orbit's placement, keeping its projection.
    pub fn apply(&self, camera: &mut Camera) {
        camera.transform = self.transform();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn camera_faces_the_target_at_the_distance() {
        let orbit = OrbitCamera::default();
        let t = orbit.transform();
        assert!((t.translation.distance(orbit.target) - orbit.distance).abs() < 1e-4);
        let to_target = (orbit.target - t.translation).normalize();
        assert!(close(t.forward(), to_target), "forward {}", t.forward());
        assert!(
            t.translation.y > orbit.target.y,
            "negative pitch looks down"
        );
    }

    #[test]
    fn orbit_keeps_target_and_distance() {
        let mut orbit = OrbitCamera::default();
        let before = orbit.transform().translation;
        orbit.orbit(Vec2::new(120.0, -40.0));
        let t = orbit.transform();
        assert!(!close(t.translation, before), "the camera moved");
        assert!((t.translation.distance(orbit.target) - orbit.distance).abs() < 1e-4);
        assert!(close(
            t.forward(),
            (orbit.target - t.translation).normalize()
        ));
    }

    #[test]
    fn pitch_is_clamped() {
        let mut orbit = OrbitCamera::default();
        orbit.orbit(Vec2::new(0.0, 1e6));
        assert_eq!(orbit.pitch, -MAX_PITCH);
        orbit.orbit(Vec2::new(0.0, -1e6));
        assert_eq!(orbit.pitch, MAX_PITCH);
    }

    #[test]
    fn pan_moves_target_in_the_view_plane() {
        let mut orbit = OrbitCamera::default();
        let forward = orbit.transform().forward();
        let start = orbit.target;
        orbit.pan(Vec2::new(50.0, 0.0));
        let moved = orbit.target - start;
        assert!(moved.length() > 0.0);
        assert!(
            moved.dot(forward).abs() < 1e-5,
            "no movement along the view"
        );
        let right = orbit.transform().right();
        assert!(
            moved.dot(right) < 0.0,
            "dragging right moves the target left"
        );

        let start = orbit.target;
        orbit.pan(Vec2::new(0.0, 50.0));
        let up = orbit.transform().up();
        assert!(
            (orbit.target - start).dot(up) > 0.0,
            "dragging down moves it up"
        );
    }

    #[test]
    fn zoom_scales_distance_within_limits() {
        let mut orbit = OrbitCamera::default();
        orbit.zoom(100.0);
        assert!(orbit.distance < 6.0, "wheel up moves closer");
        orbit.zoom(-200.0);
        assert!(orbit.distance > 6.0, "wheel down moves away");
        orbit.zoom(1e6);
        assert_eq!(orbit.distance, MIN_DISTANCE);
        orbit.zoom(-1e6);
        assert_eq!(orbit.distance, MAX_DISTANCE);
    }

    #[test]
    fn apply_sets_the_camera_transform() {
        let orbit = OrbitCamera::default();
        let mut camera = Camera {
            transform: Transform::IDENTITY,
            projection: shin::math::Projection::Perspective {
                fov_y_radians: 1.0,
                near: 0.1,
                far: 10.0,
            },
        };
        orbit.apply(&mut camera);
        assert_eq!(camera.transform, orbit.transform());
    }
}
