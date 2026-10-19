//! Plugins have no ambient authority. Only bounded value messages cross the boundary.
#![deny(unsafe_code)]
mod os_sandbox;
mod process;
mod protocol;
mod runtime;
pub use os_sandbox::{Confinement, confine_worker};
pub use process::{DataAnswer, EventError, PluginProcess, run_worker};
pub use protocol::*;
pub use runtime::{Dispatch, LuaSandbox};
