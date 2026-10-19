use super::App;
use crate::msg::Msg;
use nanatype_core::Cmd;
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
