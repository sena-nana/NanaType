use super::App;
use crate::msg::Msg;
use nanatype_core::{Cmd, Core, Effects};
use nanatype_model::outgoing;
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
            Msg::ClearReply => {
                self.run(Cmd::SetReply(None));
                self.ui.chat_target.set(None);
            }
            Msg::ClearChat => {
                self.chat.clear();
                self.ui.chat_target.set(None);
                self.publish_chat();
            }
            _ => unreachable!("not a chat message"),
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
