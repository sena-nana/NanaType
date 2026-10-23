//! Every event NanaType declares has a unique code and name, sits in one of
//! NanaType's domains and is named after it (docs/logging.md).

use std::collections::HashSet;

use nanatype_diag::EventDescriptor;

fn catalog() -> Vec<&'static EventDescriptor> {
    let mut all = nanatype::diag::all_events();
    for events in [
        nanatype_diag::EVENTS,
        nanatype_frame::diag::EVENTS,
        nanatype_twitch::diag::EVENTS,
        nanatype_p2p::diag::EVENTS,
        nanatype_plugins::diag::EVENTS,
    ] {
        all.extend_from_slice(events);
    }
    #[cfg(feature = "updates")]
    all.extend_from_slice(nanatype_update::diag::EVENTS);
    all
}

#[test]
fn events_have_unique_codes_and_names_in_their_domain() {
    let mut codes = HashSet::new();
    let mut names = HashSet::new();
    for event in catalog() {
        let prefix = nanatype_diag::domains::prefix(event.domain)
            .unwrap_or_else(|| panic!("{} is outside NanaType's domains", event.name));
        assert!(
            event.name.starts_with(prefix),
            "{} is not {prefix}*",
            event.name
        );
        assert!(
            codes.insert((event.domain, event.id)),
            "{} reuses its code",
            event.name
        );
        assert!(names.insert(event.name), "{} is declared twice", event.name);
    }
}
