//! The engine actor: one OS thread that owns the [`Headless`] engine and the
//! [`ToolRegistry`] and runs tool calls one at a time.
//!
//! `Headless` is not `Send` (its `Resources` hold `Box<dyn Any>`), so it
//! cannot live behind a tokio mutex. Instead it is created on, and never
//! leaves, a dedicated thread. Async code sends a call over a std channel
//! and awaits the reply on a tokio oneshot, so long renders and readbacks
//! never block the tokio runtime, and all wgpu work stays on one thread.

use std::sync::mpsc;
use std::thread;

use anyhow::Context as _;
use serde_json::Value;
use shin::headless::Headless;
use shin::tool::{ToolError, ToolOutput, ToolRegistry};
use tokio::sync::oneshot;

/// One tool call for the engine thread.
struct Call {
    name: String,
    args: Value,
    reply: oneshot::Sender<Result<ToolOutput, ToolError>>,
}

/// A cloneable handle to the engine thread.
#[derive(Clone)]
pub struct EngineHandle {
    calls: mpsc::Sender<Call>,
}

impl EngineHandle {
    /// Spawns the engine thread, creates the [`Headless`] engine on it with
    /// `create`, and waits until creation succeeded or failed.
    pub fn spawn(
        registry: ToolRegistry,
        create: impl FnOnce() -> anyhow::Result<Headless> + Send + 'static,
    ) -> anyhow::Result<Self> {
        let (calls, inbox) = mpsc::channel::<Call>();
        let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<()>>();

        thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                let mut engine = match create() {
                    Ok(engine) => {
                        let _ = ready_tx.send(Ok(()));
                        engine
                    }
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return;
                    }
                };
                // Ends when every `EngineHandle` (sender) has been dropped.
                for call in inbox {
                    let result = registry.call(&mut engine, &call.name, call.args);
                    // The caller may have gone away (cancelled request); fine.
                    let _ = call.reply.send(result);
                }
                log::debug!("engine thread: all handles dropped, exiting");
            })
            .context("failed to spawn the engine thread")?;

        ready_rx
            .recv()
            .context("engine thread exited before reporting readiness")?
            .context("failed to create the headless engine")?;
        Ok(Self { calls })
    }

    /// Runs tool `name` with `args` on the engine thread and awaits the result.
    pub async fn call(&self, name: String, args: Value) -> Result<ToolOutput, ToolError> {
        let (reply, result) = oneshot::channel();
        self.calls
            .send(Call { name, args, reply })
            .map_err(|_| anyhow::anyhow!("the engine thread has stopped"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("the engine thread dropped the call"))?
    }
}
