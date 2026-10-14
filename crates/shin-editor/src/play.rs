//! Play mode in the editor (GE-024): the clock that steps the simulation
//! while playing, and the demo system `--demo-orbit` registers.
//!
//! The mode itself lives in the engine's [`shin::headless::PlayState`]
//! resource, so Play from the toolbar and `sim.play` from the console (or an
//! agent) are the same thing. While playing, entities' Rhai scripts run
//! (GE-011: the engine's default schedule holds `shin::script::script_system`).
//! A project's compiled Rust systems do not run here; `--demo-orbit`
//! registers [`orbit_demo`] as a stand-in for them.

use std::time::Instant;

use shin::app::FixedTimestep;
use shin::ecs::{Name, Resources, Time, World};
use shin::headless::Headless;
use shin::math::{Transform, Vec3};

/// The name of the entities [`orbit_demo`] moves; the starter scene has one.
pub const ORBITER: &str = "moon";

/// Fixed system: puts every entity named [`ORBITER`] on a circle of radius
/// 1.6 at height 0.5 around the Y axis, one radian per simulated second,
/// like the starter game's own `orbit_system`.
pub fn orbit_demo(world: &mut World, res: &mut Resources) {
    let t = res.expect::<Time>().elapsed as f32;
    for (name, transform) in world.query_mut::<(&Name, &mut Transform)>() {
        if name.0 == ORBITER {
            transform.translation = Vec3::new(1.6 * t.cos(), 0.5, -1.6 * t.sin());
        }
    }
}

/// Steps the engine's fixed systems from wall-clock frame times while it
/// is playing and not paused, with its own [`FixedTimestep`] accumulator.
#[derive(Debug)]
pub struct PlayClock {
    timestep: FixedTimestep,
    last_frame: Option<Instant>,
}

impl PlayClock {
    /// A clock stepping at `fixed_dt` seconds.
    pub fn new(fixed_dt: f64) -> Self {
        Self {
            timestep: FixedTimestep::new(fixed_dt),
            last_frame: None,
        }
    }

    /// Called once per editor frame: measures the time since the previous
    /// frame and [`Self::advance`]s by it.
    pub fn tick(&mut self, engine: &mut Headless) {
        let now = Instant::now();
        let frame_time = self
            .last_frame
            .replace(now)
            .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
        self.advance(engine, frame_time);
    }

    /// Adds `frame_time` seconds and runs the fixed steps that are due, if
    /// the engine is playing and not paused; otherwise does nothing, so no
    /// time builds up while editing or paused. Returns the steps run.
    pub fn advance(&mut self, engine: &mut Headless, frame_time: f64) -> u32 {
        let state = engine.play_state();
        if !state.is_playing() || state.is_paused() {
            return 0;
        }
        if self.timestep.fixed_dt() != engine.fixed_dt {
            self.timestep = FixedTimestep::new(engine.fixed_dt);
        }
        let steps = self.timestep.advance(frame_time).steps;
        engine.step(steps);
        steps
    }

    /// Drops any partial step, so a new play session starts on a whole step.
    pub fn reset(&mut self) {
        self.timestep = FixedTimestep::new(self.timestep.fixed_dt());
    }
}
