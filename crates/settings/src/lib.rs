//! Persistent settings and window geometry.

#![forbid(unsafe_code)]

mod geometry;
mod migrate;
mod schema;
mod store;

#[cfg(test)]
mod tests;

pub use geometry::{
    GeometryStore, INPUT_CHAT_DEFAULT, INPUT_CHAT_MIN, INPUT_SIDEBAR_DEFAULT, INPUT_SIDEBAR_MAX,
    INPUT_SIDEBAR_MIN, InputDockLayout, InputDockTab, MonitorInfo, MonitorKey, Placement,
    SavedFrame, input_sidebar_width, layout_signature, monitor_for,
};
pub use migrate::{MigrateError, migrate};
pub use schema::*;
pub use store::{
    LoadNote, Loaded, load_json, load_settings, peek_diagnostics, peek_performance, save_json,
    write_atomic,
};
