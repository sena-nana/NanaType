use super::App;
use crate::msg::StickerMsg;
use std::sync::Arc;

impl App {
    /// Sticker packs and the Telegram importer.
    pub(super) fn sticker_msg(&mut self, msg: StickerMsg) {
        match msg {
            StickerMsg::ImportFolders(paths) => self.import_sticker_folders(paths),
            StickerMsg::Telegram(command) => {
                if matches!(command, crate::stickers::telegram::Command::Cancel) {
                    let _ = self.telegram.send(command);
                } else if !self.ui.telegram.with_untracked(|s| s.busy) {
                    let mut state = (*self.ui.telegram.get_untracked()).clone();
                    state.busy = true;
                    state.error = None;
                    self.ui.telegram.set(Arc::new(state));
                    if let Err(error) = self.telegram.send(command) {
                        let mut state = (*self.ui.telegram.get_untracked()).clone();
                        state.busy = false;
                        state.error = Some(error);
                        self.ui.telegram.set(Arc::new(state));
                    }
                }
            }
            StickerMsg::TelegramEvent(event) => match event {
                crate::stickers::telegram::Event::State(state) => {
                    self.ui.telegram.set(Arc::new(state))
                }
                crate::stickers::telegram::Event::Downloaded(root) => {
                    self.import_sticker_folders(vec![root])
                }
            },
            StickerMsg::AssetsChanged => {
                self.sticker_manager
                    .rebuild(&self.settings.assets.sticker_dirs);
                self.publish_sticker_packs();
            }
            StickerMsg::Event(event) => {
                let completed = matches!(&event, crate::stickers::Event::Done { .. });
                if let Some(root) = self.sticker_manager.accept(event) {
                    let path = root.display().to_string();
                    if !self
                        .settings
                        .assets
                        .sticker_dirs
                        .iter()
                        .any(|d| crate::stickers::folder(std::path::Path::new(d)) == root)
                    {
                        self.edit(|s| s.assets.sticker_dirs.push(path));
                    }
                }
                if completed {
                    self.refresh_assets();
                } else {
                    self.publish_sticker_packs();
                }
            }
            StickerMsg::Download(preset) => {
                if let Some(root) = self
                    .sticker_manager
                    .rows
                    .iter()
                    .find(|r| r.preset == Some(preset))
                    .map(|r| r.root.clone())
                {
                    self.sticker_manager.start(&root, Some(preset));
                    self.publish_sticker_packs();
                }
            }
            StickerMsg::Retry(root) => {
                self.sticker_manager.start(&root, None);
                self.publish_sticker_packs();
            }
            StickerMsg::Repository(root) => {
                if let Some(url) = self
                    .sticker_manager
                    .rows
                    .iter()
                    .find(|r| r.root == root)
                    .and_then(|r| r.repository.as_deref())
                    .filter(|url| {
                        url.starts_with("https://") && !url.chars().any(char::is_whitespace)
                    })
                {
                    crate::services::open_url(url);
                }
            }
            StickerMsg::Remove(root) => {
                self.sticker_manager.remove(&root);
                self.edit(|s| {
                    s.assets
                        .sticker_dirs
                        .retain(|d| crate::stickers::folder(std::path::Path::new(d)) != root)
                });
                self.refresh_assets();
            }
        }
    }
}
