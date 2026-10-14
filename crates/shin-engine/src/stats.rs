//! Frame-time statistics. Owned by GE-009.
//!
//! [`FrameStats`] keeps the most recent frame times in a fixed-size ring
//! buffer and reports their mean, median, and 99th percentile.
//! [`frame_stats_system`] feeds it from [`Time::frame_dt`] every frame;
//! [`crate::app::EcsGame`] inserts the resource and runs the system after
//! all other frame systems. [`register_tools`] adds the `stats.summary`
//! tool.

use std::collections::VecDeque;

use crate::ecs::{Resources, Time, World};
use crate::headless::Headless;
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Number of samples [`FrameStats::default`] keeps (four seconds at 60 fps).
pub const DEFAULT_CAPACITY: usize = 240;

/// The last `cap` frame times, in seconds, with summary statistics in
/// milliseconds. Pushing to a full buffer drops the oldest sample.
#[derive(Clone, Debug)]
pub struct FrameStats {
    samples: VecDeque<f64>,
    cap: usize,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl FrameStats {
    /// An empty buffer holding at most `cap` samples.
    ///
    /// # Panics
    ///
    /// If `cap` is zero.
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "FrameStats capacity must be positive");
        Self {
            samples: VecDeque::with_capacity(cap),
            cap,
        }
    }

    /// Records one frame time in seconds, dropping the oldest sample if the
    /// buffer is full.
    pub fn push(&mut self, frame_dt: f64) {
        if self.samples.len() == self.cap {
            self.samples.pop_front();
        }
        self.samples.push_back(frame_dt);
    }

    /// Number of samples currently held (at most the capacity).
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// `true` if no frame time has been recorded.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Mean frame time in milliseconds; 0 when empty.
    pub fn mean_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        1000.0 * self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }

    /// Median frame time in milliseconds (nearest-rank); 0 when empty.
    pub fn p50_ms(&self) -> f64 {
        self.percentile_ms(50.0)
    }

    /// 99th-percentile frame time in milliseconds (nearest-rank); 0 when
    /// empty.
    pub fn p99_ms(&self) -> f64 {
        self.percentile_ms(99.0)
    }

    /// Frames per second from the mean frame time; 0 when empty.
    pub fn fps(&self) -> f64 {
        let mean = self.mean_ms();
        if mean > 0.0 { 1000.0 / mean } else { 0.0 }
    }

    /// One line, for example `mean 4.1ms p50 4.0ms p99 6.2ms (243 fps)`.
    pub fn summary(&self) -> String {
        format!(
            "mean {:.1}ms p50 {:.1}ms p99 {:.1}ms ({:.0} fps)",
            self.mean_ms(),
            self.p50_ms(),
            self.p99_ms(),
            self.fps()
        )
    }

    /// The nearest-rank `p`th percentile: the smallest sample with at
    /// least `p` percent of the samples at or below it.
    fn percentile_ms(&self, p: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.samples.iter().copied().collect();
        sorted.sort_by(f64::total_cmp);
        let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
        1000.0 * sorted[rank.clamp(1, sorted.len()) - 1]
    }
}

/// Frame system: pushes [`Time::frame_dt`] into the [`FrameStats`]
/// resource, inserting a default one if it is missing.
///
/// A `frame_dt` of zero (the first frame, which has no predecessor, or a
/// headless frame) is not a real measurement and is skipped.
pub fn frame_stats_system(_world: &mut World, res: &mut Resources) {
    let frame_dt = res.get::<Time>().map_or(0.0, |time| time.frame_dt);
    if frame_dt <= 0.0 {
        return;
    }
    if res.get::<FrameStats>().is_none() {
        res.insert(FrameStats::default());
    }
    res.expect_mut::<FrameStats>().push(frame_dt);
}

/// Arguments of `stats.summary`: none. Unknown fields are rejected.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

/// Registers the `stats.summary` tool: returns [`FrameStats::summary`] as
/// text, or fails naming the missing resource if no [`FrameStats`] is
/// installed.
///
/// Headless frames have `frame_dt == 0` and are not recorded, so on a
/// [`Headless`] engine the summary reports zeros until frame times are
/// pushed.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "stats.summary",
        "Reports frame-time statistics over the last frames: mean, p50 and p99 \
         frame time in milliseconds, and frames per second.",
        |engine: &mut Headless, _: NoArgs| {
            let stats = engine.resources.get::<FrameStats>().ok_or_else(|| {
                anyhow::anyhow!("missing resource `{}`", std::any::type_name::<FrameStats>())
            })?;
            Ok(ToolOutput::Text(stats.summary()))
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 samples of 1..=100 ms, in shuffled order.
    fn one_to_hundred_ms() -> FrameStats {
        let mut stats = FrameStats::new(100);
        for i in 0..100u64 {
            // 37 is coprime with 100, so this visits every value once.
            let ms = (i * 37) % 100 + 1;
            stats.push(ms as f64 / 1000.0);
        }
        stats
    }

    #[test]
    fn percentiles_on_known_distribution() {
        let stats = one_to_hundred_ms();
        assert_eq!(stats.len(), 100);
        assert!(
            (stats.p50_ms() - 50.0).abs() < 1e-9,
            "p50 {}",
            stats.p50_ms()
        );
        assert!(
            (stats.p99_ms() - 99.0).abs() < 1e-9,
            "p99 {}",
            stats.p99_ms()
        );
        assert!((stats.mean_ms() - 50.5).abs() < 1e-9);
        assert!((stats.fps() - 1000.0 / 50.5).abs() < 1e-9);
    }

    #[test]
    fn ring_buffer_never_exceeds_cap() {
        let mut stats = FrameStats::new(8);
        for i in 0..100 {
            stats.push(f64::from(i));
            assert!(stats.len() <= 8);
        }
        assert_eq!(stats.len(), 8);
        // Only the newest eight (92..=99 s) remain.
        assert_eq!(stats.mean_ms(), 1000.0 * 95.5);
    }

    #[test]
    fn default_capacity_is_240() {
        let mut stats = FrameStats::default();
        for _ in 0..1000 {
            stats.push(0.004);
        }
        assert_eq!(stats.len(), DEFAULT_CAPACITY);
        assert_eq!(DEFAULT_CAPACITY, 240);
    }

    #[test]
    fn empty_stats_are_zero() {
        let stats = FrameStats::default();
        assert!(stats.is_empty());
        assert_eq!(stats.mean_ms(), 0.0);
        assert_eq!(stats.p50_ms(), 0.0);
        assert_eq!(stats.p99_ms(), 0.0);
        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.summary(), "mean 0.0ms p50 0.0ms p99 0.0ms (0 fps)");
    }

    #[test]
    fn summary_format() {
        let mut stats = FrameStats::new(4);
        for ms in [4.0, 4.0, 4.0, 6.0] {
            stats.push(ms / 1000.0);
        }
        assert_eq!(stats.summary(), "mean 4.5ms p50 4.0ms p99 6.0ms (222 fps)");
    }

    #[test]
    fn system_pushes_frame_dt_and_skips_zero() {
        let mut world = World::new();
        let mut res = Resources::new();
        res.insert(Time::default());

        frame_stats_system(&mut world, &mut res);
        assert!(
            res.get::<FrameStats>().is_none_or(FrameStats::is_empty),
            "zero frame_dt is skipped"
        );

        res.expect_mut::<Time>().frame_dt = 0.016;
        frame_stats_system(&mut world, &mut res);
        frame_stats_system(&mut world, &mut res);
        let stats = res.expect::<FrameStats>();
        assert_eq!(stats.len(), 2);
        assert!((stats.mean_ms() - 16.0).abs() < 1e-9);
    }

    #[test]
    fn summary_tool_through_registry() -> anyhow::Result<()> {
        use crate::tool::ToolError;

        let mut registry = ToolRegistry::new();
        register_tools(&mut registry);
        let mut engine = Headless::new(16, 16)?;

        let output = registry.call(&mut engine, "stats.summary", serde_json::json!({}))?;
        let ToolOutput::Text(text) = output else {
            panic!("expected text, got {output:?}");
        };
        assert_eq!(
            text,
            FrameStats::default().summary(),
            "Headless installs FrameStats"
        );

        for ms in [4.0, 4.0, 4.0, 6.0] {
            engine
                .resources
                .expect_mut::<FrameStats>()
                .push(ms / 1000.0);
        }
        let output = registry.call(&mut engine, "stats.summary", serde_json::Value::Null)?;
        let ToolOutput::Text(text) = output else {
            panic!("expected text, got {output:?}");
        };
        assert_eq!(text, "mean 4.5ms p50 4.0ms p99 6.0ms (222 fps)");

        engine.resources = Resources::new();
        let err = registry
            .call(&mut engine, "stats.summary", serde_json::json!({}))
            .unwrap_err();
        assert!(matches!(err, ToolError::Failed(_)), "{err:?}");
        assert!(err.to_string().contains("FrameStats"), "{err}");
        Ok(())
    }
}
