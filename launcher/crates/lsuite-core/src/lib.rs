//! The lsuite launcher's core: everything the window, `lsuite-cli` and `lsuite-mcp` can do is a
//! command in [`registry`] (`family.verb`, JSON in, JSON out), run against one [`Launcher`].
//!
//! - **apps**: the lsuite apps, installed, updated, removed and opened from their signed GitHub
//!   releases (`catalog`, `release`, `install`).
//! - **account**: the lsuite account every app shares (`~/.lsuite/account.json`), and its lsuite Pass.
//! - **cloud**: lsuite Cloud, the storage that comes with lsuite Pass.
//! - **market**: the lsuite Marketplace, plugins by the people who use lsuite (with lsuite Pass).
//! - **agent**: the lsuite agent, for jobs that span the apps (HARNESS.md, part 8).
//!
//! Long work reports progress on the [`events::Hub`]; the window draws it, the CLI prints it.

pub mod account;
pub mod agent;
pub mod apps;
pub mod catalog;
pub mod cloud;
pub mod events;
pub mod install;
pub mod market;
pub mod paths;
pub mod platform;
pub mod registry;
pub mod release;
pub mod selfupdate;
pub mod settings;
pub mod sync;
pub mod util;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

pub use events::{Event, Hub};
pub use platform::Platform;
pub use registry::{Source, call};

pub type CmdResult<T = Value> = Result<T, String>;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The launcher's state while it runs: the event stream, what the last release check found, and
/// which apps have work going on.
pub struct Launcher {
    pub hub: Hub,
    pub platform: Platform,
    pub(crate) latest: Mutex<BTreeMap<String, apps::Latest>>,
    pub(crate) busy: Mutex<BTreeSet<String>>,
    pub(crate) sign_in: Mutex<Option<tokio::task::AbortHandle>>,
    /// Apps the site lists that this build doesn't know (see `apps::check`).
    pub(crate) others: Mutex<Vec<Value>>,
    pub(crate) update: Mutex<selfupdate::State>,
    /// Synced folders with a sync running.
    pub(crate) syncing: Mutex<BTreeSet<String>>,
    /// The lsuite agent's conversation.
    pub(crate) agent: Mutex<agent::State>,
}

impl Launcher {
    pub fn new() -> Arc<Self> {
        Arc::new(Launcher { hub: Hub::default(), platform: Platform::current(), latest: Mutex::new(apps::load_latest()), busy: Mutex::default(), sign_in: Mutex::default(), others: Mutex::default(), update: Mutex::default(), syncing: Mutex::default(), agent: Mutex::default() })
    }

    /// Apps with work going on (install, update, removal).
    pub fn busy(&self) -> BTreeSet<String> {
        self.busy.lock().clone()
    }
}

/// The app's icon (256 px PNG) for the window and the Linux app menu.
pub fn app_icon(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "ryolune" => include_bytes!("../assets/icons/ryolune.png"),
        "kimchi" => include_bytes!("../assets/icons/kimchi.png"),
        "zenith" => include_bytes!("../assets/icons/zenith.png"),
        "nori" => include_bytes!("../assets/icons/nori.png"),
        "folio" => include_bytes!("../assets/icons/folio.png"),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
