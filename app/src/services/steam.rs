//! The Steam connection made at launch, kept until the app starts and takes
//! it. Worker processes and the packager's self-check never connect.
use std::sync::Mutex;

use nanatype_steam::{Bootstrap, Steam};

use crate::workshop::Availability;

static BOOT: Mutex<Option<Bootstrap>> = Mutex::new(None);

/// Connect to the Steam client. Call once the process is the app's single
/// instance, before other threads start (Steam sets `SteamAppId`).
pub fn connect() {
    if std::env::var_os(nana_ui::SELF_CHECK_ENV).is_some() {
        return;
    }
    let boot = nanatype_steam::bootstrap();
    *BOOT.lock().unwrap() = Some(boot);
}

/// The connection, if any, and what the Workshop pages should say.
pub fn take() -> (Option<Steam>, Availability) {
    match BOOT.lock().unwrap().take() {
        Some(Bootstrap::Ready(steam)) => (Some(steam), Availability::Ready),
        Some(Bootstrap::Unavailable(why)) => {
            // Logged here: `connect` runs before the session log exists.
            nanatype_diag::report!(crate::diag::STEAM_UNAVAILABLE; "{why}");
            (None, Availability::Unavailable(why))
        }
        // A debug Steam build that was not asked to start Steam.
        _ if crate::distribution::STEAM => (
            None,
            Availability::Unavailable(format!("{}=1", nanatype_steam::OPT_IN_ENV)),
        ),
        _ => (None, Availability::Off),
    }
}
