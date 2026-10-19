//! Plugins have no ambient authority. Only bounded value messages cross the boundary.
#![deny(unsafe_code)]
mod process;
mod protocol;
mod runtime;
pub use process::{EventError, PluginProcess, run_worker};
pub use protocol::*;
pub use runtime::LuaSandbox;
