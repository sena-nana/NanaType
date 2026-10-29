use super::App;
use crate::msg::Msg;
use nanatype_core::{Cmd, Core, Effects};
use nanatype_model::outgoing;
use nanatype_settings::InputDockLayout;
use std::sync::Arc;

impl App {
    /// The live chat panel.
    pub(super) fn chat_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Chat(message) => {
                if self.plugins.has_running() {
                    self.plugins.broadcast(serde_json::json!({"type":"chat_message","platform":message.platform.source(),"author":message.user.name,"text":message.doc.plain_text()}),self.ui);
                }
                self.chat_message(message);
            }
            Msg::ChatAssetReady(asset) => {
                let mut urls = (*self.ui.asset_urls.get_untracked()).clone();
                urls.remove(&asset.to_string());
                self.ui.asset_urls.set(Arc::new(urls));
                self.ensure_urls([asset]);
            }
            Msg::ChatMention { seq, reply } => self.mention(seq, reply),
            Msg::ChatSelect(step) => self.select_chat(step),
            Msg::ChatMentionSelected { reply } => {
                let shown = self.chat_shown();
                let selected = self
                    .ui
                    .chat_target
                    .get_untracked()
                    .filter(|seq| shown.contains(seq));
                match selected.or_else(|| shown.last().copied()) {
                    Some(seq) => self.mention(seq, reply),
                    None => self.notice(self.ui.now_t("chat-none"), false),
                }
            }
            Msg::ClearReply => {
                self.run(Cmd::SetReply(None));
                self.ui.chat_target.set(None);
            }
            Msg::ClearChat => {
                self.chat.clear();
                self.ui.chat_target.set(None);
                self.publish_chat();
            }
            Msg::Dock(layout) => self.dock(layout),
            Msg::ToggleChat => {
                let layout = self.ui.input_dock.get_untracked().toggle_chat();
                self.dock(layout);
            }
            Msg::DockTab(tab) => {
                let layout = self.ui.input_dock.get_untracked().rail(tab);
                self.dock(layout);
            }
            Msg::ShowChat => {
                let layout = self.ui.input_dock.get_untracked();
                if !layout.chat_visible() {
                    self.dock(layout.toggle_chat());
                }
                self.update(Msg::ShowWindow(crate::windows::WindowKind::Input));
            }
            _ => unreachable!("not a chat message"),
        }
    }

    /// Move the keyboard's chat selection, which the panel highlights, and
    /// bring chat into view for it.
    fn select_chat(&mut self, step: i32) {
        let shown = self.chat_shown();
        if shown.is_empty() {
            self.notice(self.ui.now_t("chat-none"), false);
            return;
        }
        let next = step_selection(&shown, self.ui.chat_target.get_untracked(), step);
        let layout = self.ui.input_dock.get_untracked();
        if !layout.chat_visible() {
            self.dock(layout.toggle_chat());
        }
        self.ui.chat_target.set(next);
    }

    /// Show the sidebar as `layout` (the sidebar may already) and keep it
    /// for the next start.
    fn dock(&mut self, layout: InputDockLayout) {
        let layout = layout.normalized();
        if self.ui.input_dock.get_untracked() != layout {
            self.ui.input_dock.set(layout);
        }
        self.save_geometry();
        self.sync_tray();
    }
}

/// The selection `step` messages on from `current` in `shown` (oldest
/// first). Up from none selects the newest; up stops at the oldest; down
/// past the newest selects none, back to following the feed.
fn step_selection(shown: &[u64], current: Option<u64>, step: i32) -> Option<u64> {
    let at = current.and_then(|seq| shown.iter().position(|s| *s == seq));
    match at {
        None => (step < 0).then(|| shown.last().copied()).flatten(),
        Some(i) => {
            let j = i as i64 + i64::from(step);
            if j < 0 {
                shown.first().copied()
            } else {
                shown.get(j as usize).copied()
            }
        }
    }
}

/// All local commands, including the editor's Send, pass this boundary before
/// consuming the draft or producing submission/history events.
pub(super) fn handle_command(
    core: &mut Core,
    cmd: Cmd,
    synchronizing: bool,
    now: u64,
) -> Result<Effects, ()> {
    if synchronizing {
        let invalid = match &cmd {
            Cmd::Send => !core.draft().is_blank() && outgoing::plain_text(core.draft()).is_err(),
            Cmd::SendMarkup(markup) => {
                let doc = nanatype_markup::parse(markup).doc;
                !doc.is_blank() && outgoing::plain_text(&doc).is_err()
            }
            Cmd::DraftChanged(doc) => {
                outgoing::image_count(doc) > outgoing::image_count(core.draft())
            }
            _ => false,
        };
        if invalid {
            return Err(());
        }
    }
    Ok(core.handle(cmd, now))
}

#[cfg(test)]
mod tests {
    use super::step_selection;

    #[test]
    fn the_keyboard_steps_through_chat_newest_first() {
        let shown = [3, 5, 8];
        assert_eq!(step_selection(&shown, None, -1), Some(8));
        assert_eq!(step_selection(&shown, None, 1), None);
        assert_eq!(step_selection(&shown, Some(8), -1), Some(5));
        assert_eq!(step_selection(&shown, Some(3), -1), Some(3));
        assert_eq!(step_selection(&shown, Some(5), 1), Some(8));
        assert_eq!(step_selection(&shown, Some(8), 1), None);
        // A selection the filter now hides starts over from the newest.
        assert_eq!(step_selection(&shown, Some(4), -1), Some(8));
        assert_eq!(step_selection(&[], None, -1), None);
    }
}
