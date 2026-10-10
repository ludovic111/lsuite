//! What the core tells its clients while it works: progress of long tasks (downloads, installs)
//! and which part of the state changed. The window listens; the CLI prints progress.

use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    /// A task advanced. `task` is stable for its whole life (`install:kimchi`, `update:lsuite`).
    #[serde(rename_all = "camelCase")]
    Progress { task: String, label: String, done: u64, total: Option<u64> },
    /// A task finished (`ok` false: `message` is the error).
    #[serde(rename_all = "camelCase")]
    TaskDone { task: String, ok: bool, message: String },
    /// Something the clients show changed: `apps`, `plugins`, `agent`, `update` or `settings`.
    Changed { what: String },
}

/// The event stream (cheap to clone; nobody listening is fine).
#[derive(Clone)]
pub struct Hub(tokio::sync::broadcast::Sender<Event>);

impl Default for Hub {
    fn default() -> Self {
        Self(tokio::sync::broadcast::channel(512).0)
    }
}

impl Hub {
    pub fn emit(&self, e: Event) {
        let _ = self.0.send(e);
    }

    pub fn changed(&self, what: &str) {
        self.emit(Event::Changed { what: what.into() });
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.0.subscribe()
    }

    /// A progress reporter for one task that emits at most every 1 % or 150 ms.
    pub fn progress(&self, task: impl Into<String>, label: impl Into<String>) -> Progress {
        Progress { hub: self.clone(), task: task.into(), label: label.into(), last: None, last_done: 0 }
    }
}

pub struct Progress {
    hub: Hub,
    task: String,
    label: String,
    last: Option<std::time::Instant>,
    last_done: u64,
}

impl Progress {
    pub fn task(&self) -> &str {
        &self.task
    }

    /// Changes what the task says it is doing ("Downloading", "Checking the signature"…), at once.
    pub fn stage(&mut self, label: impl Into<String>, done: u64, total: Option<u64>) {
        self.label = label.into();
        self.last = None;
        self.update(done, total);
    }

    pub fn update(&mut self, done: u64, total: Option<u64>) {
        let now = std::time::Instant::now();
        let step = total.map_or(256 * 1024, |t| (t / 100).max(1));
        let due = self.last.is_none_or(|l| now.duration_since(l).as_millis() >= 150) || done.saturating_sub(self.last_done) >= step || total == Some(done);
        if !due {
            return;
        }
        self.last = Some(now);
        self.last_done = done;
        self.hub.emit(Event::Progress { task: self.task.clone(), label: self.label.clone(), done, total });
    }

    pub fn finish(&self, result: &Result<impl Sized, String>, ok_message: impl Into<String>) {
        let (ok, message) = match result {
            Ok(_) => (true, ok_message.into()),
            Err(e) => (false, e.clone()),
        };
        self.hub.emit(Event::TaskDone { task: self.task.clone(), ok, message });
    }
}
