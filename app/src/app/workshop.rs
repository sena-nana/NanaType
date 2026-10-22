//! Steam Workshop: installed subscriptions are read-only package folders
//! for the registry; browsing opens the Workshop in the Steam overlay, or
//! through the Steam client when the overlay is off.
use super::App;
use crate::msg::Msg;
use nanatype_steam::Event;

impl App {
    pub(super) fn workshop_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Workshop(Event::Items(items)) => {
                if self.workshop.set_items(items) {
                    self.rescan_workshop();
                }
                self.publish_workshop();
            }
            Msg::Workshop(Event::Details(details)) => {
                self.workshop.set_details(details);
                self.publish_workshop();
            }
            Msg::Workshop(Event::Unsubscribed { item, result }) => match result {
                Ok(()) => self.unsubscribed(item),
                Err(message) => {
                    let text = self.ui.tr(
                        "workshop-unsubscribe-failed",
                        &[("message", message.into())],
                    );
                    self.notice(text, true);
                }
            },
            Msg::WorkshopBrowse(tag) => {
                self.open_workshop(&nanatype_steam::browse_url(Some(tag.name())));
            }
            Msg::WorkshopOpen(item) => self.open_workshop(&nanatype_steam::item_url(item)),
            Msg::WorkshopUnsubscribe(item) => self.steam.unsubscribe(item),
            Msg::WorkshopDownload(item) => self.steam.download(item),
            _ => unreachable!("not a Workshop message"),
        }
    }

    fn open_workshop(&self, url: &str) {
        if !self.steam.overlay(url) {
            crate::services::open_url(&nanatype_steam::client_url(url));
        }
    }

    /// Installed content changed: point the registry at the current folders
    /// and scan again, whether or not the folders themselves changed.
    fn rescan_workshop(&mut self) {
        self.registry.set_external(self.workshop.roots());
        self.request_skin_reload(true);
    }

    /// A registry that came back from a background scan or install was
    /// cloned before the latest subscriptions arrived: catch it up.
    pub(super) fn reapply_workshop(&mut self) {
        if self.registry.set_external(self.workshop.roots()) {
            self.request_skin_reload(true);
        }
    }

    pub(super) fn publish_workshop(&self) {
        if self.workshop.availability != crate::workshop::Availability::Off {
            self.ui.workshop.set(self.workshop.view(&self.registry));
        }
    }

    /// The user unsubscribed: the item goes now (Steam deletes its files at
    /// exit), and so does any choice that named it.
    fn unsubscribed(&mut self, item: u64) {
        let package = self
            .registry
            .external_status()
            .iter()
            .find(|s| s.item == item && s.problem.is_none())
            .and_then(|s| s.id.clone());
        self.workshop.forget(item);
        self.rescan_workshop();
        self.publish_workshop();
        if let Some(id) = package {
            if self.settings.skin.id == id {
                self.edit(|s| s.skin.id = nanatype_theme::DEFAULT_SKIN.into());
            }
            if self.settings.sound.pack.as_ref() == Some(&id) {
                self.edit(|s| s.sound.pack = None);
            }
        }
    }
}
