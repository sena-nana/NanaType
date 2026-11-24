//! Media asset resolution, sticker library/watchers, and reactive `asset_urls` cache.

use std::path::PathBuf;
use std::sync::Arc;

use nanatype_model::{AssetRef, Document, HistoryItem};
use nanatype_theme::LoadedSkin;

use super::draft::doc_assets;
use crate::assets::AssetLibrary;
use crate::chat::ChatMessage;
use crate::ui::Ui;

pub struct MediaHub {
    pub assets: AssetLibrary,
    pub sticker_manager: crate::stickers::Manager,
    pub telegram: crate::stickers::telegram::Service,
    pub sticker_watch: Option<crate::services::watch::Watch>,
    /// Current loaded skin files for `AssetRef::Skin` resolution.
    pub loaded_skin: Option<LoadedSkin>,
}

impl MediaHub {
    pub fn new(
        blobs_dir: PathBuf,
        sticker_manager: crate::stickers::Manager,
        telegram: crate::stickers::telegram::Service,
        loaded_skin: LoadedSkin,
    ) -> Self {
        Self {
            assets: AssetLibrary::new(blobs_dir),
            sticker_manager,
            telegram,
            sticker_watch: None,
            loaded_skin: Some(loaded_skin),
        }
    }

    pub fn url(&self, asset: &AssetRef) -> Option<String> {
        self.assets.url(asset, self.loaded_skin.as_ref())
    }

    pub fn resolve(&self, asset: &AssetRef) -> Option<PathBuf> {
        self.assets.resolve(asset, self.loaded_skin.as_ref())
    }

    /// Give every asset of `assets` a URL the views can load.
    pub fn ensure_urls(&self, assets: impl IntoIterator<Item = AssetRef>, ui: Ui) {
        let urls = ui.asset_urls.get_untracked();
        let mut updates: Vec<(String, String)> = Vec::new();
        for asset in assets {
            let key = asset.to_string();
            if urls.contains_key(&key) || updates.iter().any(|(k, _)| k == &key) {
                continue;
            }
            if let Some(url) = self.url(&asset) {
                updates.push((key, url));
            }
        }
        if updates.is_empty() {
            return;
        }
        let mut next = (*urls).clone();
        next.extend(updates);
        ui.asset_urls.set(Arc::new(next));
    }

    /// Invalidate `asset` and resolve its fresh URL in a single copy-on-write
    /// update to `ui.asset_urls`.
    pub fn invalidate_and_resolve_asset(&self, asset: AssetRef, ui: Ui) {
        let key = asset.to_string();
        let resolved = self.url(&asset);
        let urls = ui.asset_urls.get_untracked();
        let had = urls.contains_key(&key);
        if !had && resolved.is_none() {
            return;
        }
        if urls.get(&key) == resolved.as_ref() {
            return;
        }
        let mut next = (*urls).clone();
        match resolved {
            Some(url) => {
                next.insert(key, url);
            }
            None => {
                next.remove(&key);
            }
        }
        ui.asset_urls.set(Arc::new(next));
    }
}

pub(crate) fn display_assets(view: &nanatype_core::DisplayView) -> Vec<AssetRef> {
    let mut out = doc_assets(&view.doc);
    out.extend(view.speaker.avatar.as_ref().and_then(|a| a.parse().ok()));
    out.extend(view.reply.as_ref().and_then(|r| r.user.avatar.clone()));
    out
}

/// A history row's pictures: its text's, its avatar (chat rows name a
/// saved one) and the avatar of the message it answers.
pub(crate) fn row_assets(item: &HistoryItem, doc: &Document) -> Vec<AssetRef> {
    let mut out = doc_assets(doc);
    out.extend(
        item.avatar
            .as_deref()
            .and_then(|a| a.parse::<AssetRef>().ok()),
    );
    out.extend(item.reply.as_ref().and_then(|r| r.user.avatar.clone()));
    out
}

pub(crate) fn message_assets(m: &ChatMessage) -> Vec<AssetRef> {
    let mut out = doc_assets(&m.doc);
    out.extend(m.user.avatar.clone());
    out
}

impl super::App {
    pub(super) fn publish_sticker_packs(&self) {
        self.ui
            .sticker_packs
            .set(Arc::new(self.media.sticker_manager.rows.clone()));
    }

    pub(super) fn refresh_assets(&mut self) {
        let previous = self.ui.asset_urls.with_untracked(|urls| {
            urls.iter()
                .filter(|(key, _)| key.starts_with("skin:"))
                .map(|(_, url)| url.clone())
                .collect::<Vec<_>>()
        });
        let locale = self.ui.locale.get_untracked();
        let mut seen = std::collections::HashSet::new();
        let mut stickers = Vec::new();
        let mut rows = Vec::new();
        let mut images = Vec::new();
        let mut urls = self
            .appearance
            .prepared_skin_urls
            .take()
            .unwrap_or_else(|| {
                crate::assets::skin_preview_urls(
                    &self.appearance.loaded.skin,
                    &*self.appearance.loaded.files,
                )
            });
        for dir in &self.settings.assets.sticker_dirs {
            let root = crate::stickers::folder(std::path::Path::new(dir));
            if let Some(pack) = self.media.sticker_manager.loaded.get(&root) {
                for (entry, image) in pack.definition.entries.iter().zip(&pack.images) {
                    let sticker = crate::assets::Sticker {
                        lib: entry.lib.clone(),
                        name: entry.name.clone(),
                        path: image.frame_path(0),
                        root: image.dir.clone(),
                    };
                    let asset = sticker.asset().to_string();
                    if !seen.insert(asset.clone()) {
                        continue;
                    }
                    if let Some(url) = sticker.url() {
                        urls.insert(asset.clone(), url.clone());
                        rows.push(crate::ui::StickerRow {
                            asset,
                            name: entry
                                .translations
                                .get(locale.tag())
                                .unwrap_or(&entry.title)
                                .clone(),
                            lib: entry.lib.clone(),
                            url,
                            height_em: entry.height_em,
                            pack_title: entry.pack_title.clone(),
                        });
                    }
                    stickers.push(sticker);
                    images.push(image.clone());
                }
            }
        }
        crate::stickers::cache::publish(images.into_iter());
        stickers.sort_by(|a, b| (&a.lib, &a.name).cmp(&(&b.lib, &b.name)));
        rows.sort_by(|a, b| (&a.lib, &a.asset).cmp(&(&b.lib, &b.asset)));
        self.media.assets.set_stickers(stickers);
        self.publish_sticker_packs();
        // Pictures of files, pasted images and chat in the current views.
        let mut wanted = doc_assets(self.session.core.draft());
        wanted.extend(display_assets(self.session.core.display()));
        for row in self
            .session
            .core
            .history()
            .iter()
            .chain(self.session.core.danmaku().iter())
        {
            wanted.extend(row_assets(&row.item, row.doc()));
        }
        for m in &self.session.chat {
            wanted.extend(message_assets(m));
        }
        for record in &self.ui.archive.get_untracked().records {
            if let Some(markup) = record.markup.as_deref() {
                wanted.extend(doc_assets(&nanatype_markup::parse(markup).doc));
            }
        }
        for asset in wanted {
            let key = asset.to_string();
            if !urls.contains_key(&key)
                && let Some(u) = self.media.url(&asset)
            {
                urls.insert(key, u);
            }
        }
        let mut portraits = std::collections::HashMap::new();
        let p = &self.appearance.skin.dialogue.portrait;
        if let Some(root) = self.appearance.loaded.files.root() {
            if let Some(url) = p
                .image
                .as_deref()
                .and_then(|img| crate::assets::skin_file_url(root, img))
            {
                portraits.insert("default".to_owned(), url);
            }
            for (name, img) in &p.variants {
                if let Some(url) = crate::assets::skin_file_url(root, img) {
                    portraits.insert(name.clone(), url);
                }
            }
        }
        self.ui.asset_urls.set(Arc::new(urls));
        self.ui.stickers.set(Arc::new(rows));
        self.ui.portraits.set(Arc::new(portraits));
        crate::ui::retire_skin_urls(self.ui, previous);
    }

    /// Resolve the pictures of the display, the draft and the newest
    /// history row.
    pub(super) fn refresh_atom_urls(&mut self) {
        let mut assets = display_assets(self.session.core.display());
        assets.extend(doc_assets(self.session.core.draft()));
        if let Some(row) = self.session.core.history().back() {
            assets.extend(row_assets(&row.item, row.doc()));
        }
        if let Some(row) = self.session.core.danmaku().back() {
            assets.extend(row_assets(&row.item, row.doc()));
        }
        self.ensure_urls(assets);
    }

    /// Give every asset of `assets` a URL the views can load.
    pub(crate) fn ensure_urls(&mut self, assets: impl IntoIterator<Item = AssetRef>) {
        self.media.ensure_urls(assets, self.ui);
    }
}
