use super::App;
use crate::msg::Msg;
use std::sync::Arc;

impl App {
    /// Sticker packs and the Telegram importer.
    pub(super) fn sticker_msg(&mut self, msg: Msg) {
        match msg {
            Msg::StickerImportFolders(paths) => self.import_sticker_folders(paths),
            Msg::Telegram(command) => {
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
            Msg::TelegramEvent(event) => match event {
                crate::stickers::telegram::Event::State(state) => {
                    self.ui.telegram.set(Arc::new(state))
                }
                crate::stickers::telegram::Event::Downloaded(root) => {
                    self.import_sticker_folders(vec![root])
                }
            },
            Msg::AssetsChanged => {
                self.sticker_manager
                    .rebuild(&self.settings.assets.sticker_dirs);
                self.publish_sticker_packs();
            }
            Msg::Stickers(event) => {
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
            Msg::StickerDownload(preset) => {
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
            Msg::StickerRetry(root) => {
                self.sticker_manager.start(&root, None);
                self.publish_sticker_packs();
            }
            Msg::StickerRepository(root) => {
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
            Msg::StickerRemove(root) => {
                self.sticker_manager.remove(&root);
                self.edit(|s| {
                    s.assets
                        .sticker_dirs
                        .retain(|d| crate::stickers::folder(std::path::Path::new(d)) != root)
                });
                self.refresh_assets();
            }
            _ => unreachable!("not a sticker message"),
        }
    }
}
