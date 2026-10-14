//! A `log` facade sink for the Log panel: every record that passes the
//! `RUST_LOG` filter goes to `env_logger` as usual and is also kept, as one
//! line, in a bounded [`LogBuffer`] the UI reads each frame.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

/// How many lines the Log panel keeps; older lines are dropped.
pub const LOG_CAPACITY: usize = 200;

/// The most recent [`LOG_CAPACITY`] log lines, shared between the logger
/// and the UI. Cloning shares the same buffer.
#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<VecDeque<String>>>);

impl LogBuffer {
    /// Appends `line`, dropping the oldest line when full.
    pub fn push(&self, line: String) {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == LOG_CAPACITY {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// A copy of the buffered lines, oldest first. The lock is released
    /// before the caller uses them, so code that logs while drawing the
    /// lines cannot deadlock.
    pub fn lines(&self) -> Vec<String> {
        let lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        lines.iter().cloned().collect()
    }
}

/// Forwards to `env_logger` and records accepted records in a [`LogBuffer`].
struct EditorLogger {
    inner: env_logger::Logger,
    buffer: LogBuffer,
}

impl log::Log for EditorLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.inner.matches(record) {
            return;
        }
        self.inner.log(record);
        self.buffer.push(format!(
            "[{:<5} {}] {}",
            record.level(),
            record.target(),
            record.args()
        ));
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// Installs the global logger: `RUST_LOG` filtering (default
/// `warn,shin=info,shin_editor=info`) to stderr, with every accepted line
/// also pushed into `buffer`.
pub fn init(buffer: LogBuffer) -> anyhow::Result<()> {
    let inner = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,shin=info,shin_editor=info"),
    )
    .build();
    log::set_max_level(inner.filter());
    log::set_boxed_logger(Box::new(EditorLogger { inner, buffer }))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_latest_lines() {
        let buffer = LogBuffer::default();
        for i in 0..LOG_CAPACITY + 5 {
            buffer.push(format!("line {i}"));
        }
        let lines = buffer.lines();
        assert_eq!(lines.len(), LOG_CAPACITY);
        assert_eq!(lines[0], "line 5");
        assert_eq!(
            lines[LOG_CAPACITY - 1],
            format!("line {}", LOG_CAPACITY + 4)
        );
    }

    #[test]
    fn clones_share_the_buffer() {
        let buffer = LogBuffer::default();
        buffer.clone().push("hello".to_string());
        assert_eq!(buffer.lines(), ["hello"]);
    }
}
