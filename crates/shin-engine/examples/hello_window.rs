//! Opens a window cleared to cornflower blue. Once a second the title
//! shows how many `fixed_update` calls ran in the last second and the
//! frame-time summary. Press Escape to quit.
//!
//! Run with `cargo run --example hello_window`.

use std::time::{Duration, Instant};

use shin::app::{self, AppConfig, Game};
use shin::stats::FrameStats;

const TITLE: &str = "hello_window";

struct HelloWindow {
    updates: u32,
    window_start: Instant,
    title: String,
    /// Frame times, measured between `render` calls.
    stats: FrameStats,
    last_frame: Option<Instant>,
}

impl Game for HelloWindow {
    fn fixed_update(&mut self, _dt: f64) {
        self.updates += 1;
    }

    fn render(
        &mut self,
        _ctx: &shin::gpu::GpuContext,
        _view: &wgpu::TextureView,
        _depth: &wgpu::TextureView,
        _alpha: f64,
    ) {
        let now = Instant::now();
        if let Some(last) = self.last_frame.replace(now) {
            self.stats.push((now - last).as_secs_f64());
        }
        if self.window_start.elapsed() >= Duration::from_secs(1) {
            self.title = format!(
                "{TITLE} - {} fixed updates/s | {}",
                self.updates,
                self.stats.summary()
            );
            self.updates = 0;
            self.window_start = Instant::now();
        }
    }

    fn title(&self) -> Option<String> {
        Some(self.title.clone())
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let config = AppConfig {
        title: TITLE.to_string(),
        // Linear RGB; the sRGB surface encodes it to cornflower blue #6495ED.
        clear_color: wgpu::Color {
            r: 0.127,
            g: 0.301,
            b: 0.846,
            a: 1.0,
        },
        ..Default::default()
    };
    let game = HelloWindow {
        updates: 0,
        window_start: Instant::now(),
        title: TITLE.to_string(),
        stats: FrameStats::default(),
        last_frame: None,
    };
    app::run(config, game)
}
