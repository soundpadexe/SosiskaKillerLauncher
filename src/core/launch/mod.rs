pub mod download;
pub mod install;
pub mod authlib;
pub mod java;
pub mod loader;
pub mod manifest;

use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Scanning,
    Preparing,
    Java,
    Libraries,
    Assets,
    Natives,
    /// Running the Forge/NeoForge installer (processors patch the game).
    LoaderInstall,
    Starting,
    Running,
    Exited,
    Failed(String),
}

/// Shared between the tokio install task and the UI thread.
pub struct Progress {
    stage: Mutex<Stage>,
    pub done: AtomicU64,
    pub total: AtomicU64,
    child: Mutex<Option<Child>>,
    killed: AtomicBool,
    guard: Mutex<Option<crate::core::guard::Report>>,
}

impl Progress {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            stage: Mutex::new(Stage::Preparing),
            done: AtomicU64::new(0),
            total: AtomicU64::new(0),
            child: Mutex::new(None),
            killed: AtomicBool::new(false),
            guard: Mutex::new(None),
        })
    }

    pub fn set_stage(&self, s: Stage) {
        self.reset_counters();
        *self.stage.lock().unwrap() = s;
    }

    pub fn reset_counters(&self) {
        self.done.store(0, Relaxed);
        self.total.store(0, Relaxed);
    }

    pub fn stage(&self) -> Stage {
        self.stage.lock().unwrap().clone()
    }

    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.load(Relaxed);
        (total > 0).then(|| (self.done.load(Relaxed) as f64 / total as f64).min(1.0) as f32)
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.stage(), Stage::Exited | Stage::Failed(_))
    }
}

impl Progress {
    /// Takes ownership of the game process and watches it from a background
    /// thread (polling, so the handle stays available for `kill`).
    pub fn supervise(self: &Arc<Self>, child: Child, on_change: impl Fn() + Send + 'static) {
        *self.child.lock().unwrap() = Some(child);
        self.set_stage(Stage::Running);
        on_change();
        let me = self.clone();
        std::thread::Builder::new()
            .name("skl-game-watch".into())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let mut guard = me.child.lock().unwrap();
                let Some(c) = guard.as_mut() else { break };
                let finished = match c.try_wait() {
                    Ok(Some(status)) if status.success() || me.killed.load(Relaxed) => Some(Stage::Exited),
                    Ok(Some(status)) => Some(Stage::Failed(format!("exit code {}", status.code().unwrap_or(-1)))),
                    Ok(None) => None,
                    Err(e) => Some(Stage::Failed(e.to_string())),
                };
                if let Some(stage) = finished {
                    *guard = None;
                    drop(guard);
                    me.set_stage(stage);
                    on_change();
                    break;
                }
            })
            .expect("spawn watcher thread");
    }

    /// Terminates the running game. The watcher thread reports `Exited`.
    pub fn set_guard(&self, r: crate::core::guard::Report) {
        *self.guard.lock().unwrap() = Some(r);
    }

    pub fn guard(&self) -> Option<crate::core::guard::Report> {
        self.guard.lock().unwrap().clone()
    }

    pub fn kill(&self) {
        self.killed.store(true, Relaxed);
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    #[cfg(test)]
    pub fn was_killed(&self) -> bool {
        self.killed.load(Relaxed)
    }
}
