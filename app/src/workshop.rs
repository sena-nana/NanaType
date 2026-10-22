//! Steam Workshop subscriptions as the app shows them: which installed items
//! the package registry reads, and one row per subscription for the Skins and
//! Playback pages. Steam reports items and their details separately; the
//! registry says what each installed folder turned out to be.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nanatype_steam::{Details, Item, ItemState};
use nanatype_theme::{ExternalProblem, ExternalRoot, PackageCapability, SkinRegistry};

/// The kind of content a Workshop browse button asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Skins,
    SoundPacks,
}

impl Tag {
    pub fn name(self) -> &'static str {
        match self {
            Self::Skins => nanatype_steam::TAG_SKIN,
            Self::SoundPacks => nanatype_steam::TAG_SOUNDS,
        }
    }
}

/// Whether the Workshop can be used in this run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Availability {
    /// Not a Steam build.
    #[default]
    Off,
    Ready,
    /// A Steam build that could not reach Steam, and why.
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Skin,
    SoundPack,
    /// Another package, or not known until it downloads.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowState {
    Ready,
    /// Percent done, once Steam knows the size.
    Downloading(Option<u8>),
    Pending,
    NotInstalled,
    /// Installed but not a package the registry can use.
    Invalid(String),
    /// Another package with the same id wins.
    Shadowed(String),
}

/// One subscription, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub item: u64,
    pub title: String,
    pub kind: Kind,
    pub state: RowState,
    /// The package id the installed copy declares.
    pub package: Option<String>,
    pub folder: Option<PathBuf>,
}

/// What the UI shows: availability and every subscription.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    pub availability: Availability,
    pub rows: Vec<Row>,
}

impl View {
    pub fn row_of(&self, package: &str) -> Option<&Row> {
        self.rows
            .iter()
            .find(|r| r.package.as_deref() == Some(package) && r.state == RowState::Ready)
    }
}

#[derive(Debug, Default)]
pub struct Workshop {
    pub availability: Availability,
    items: Vec<Item>,
    details: HashMap<u64, Details>,
}

impl Workshop {
    pub fn new(availability: Availability, items: Vec<Item>) -> Self {
        Self {
            availability,
            items,
            details: HashMap::new(),
        }
    }

    /// The installed folders the registry should read. An item with an
    /// update waiting still has its previous version installed.
    pub fn roots(&self) -> Vec<ExternalRoot> {
        self.items
            .iter()
            .filter(|i| matches!(i.state, ItemState::Installed | ItemState::NeedsUpdate))
            .filter_map(|i| {
                i.folder
                    .clone()
                    .map(|folder| ExternalRoot { folder, item: i.id })
            })
            .collect()
    }

    /// Replace the subscriptions; true when installed content changed and
    /// the registry must scan again (a new folder, or an update in place).
    pub fn set_items(&mut self, items: Vec<Item>) -> bool {
        let installed = |items: &[Item]| -> Vec<(u64, Option<PathBuf>, u32)> {
            items
                .iter()
                .filter(|i| matches!(i.state, ItemState::Installed | ItemState::NeedsUpdate))
                .map(|i| (i.id, i.folder.clone(), i.timestamp))
                .collect()
        };
        let changed = installed(&items) != installed(&self.items);
        self.items = items;
        changed
    }

    pub fn set_details(&mut self, details: Vec<Details>) {
        self.details.extend(details.into_iter().map(|d| (d.id, d)));
    }

    /// Drop an unsubscribed item now; Steam deletes its files at exit.
    pub fn forget(&mut self, item: u64) {
        self.items.retain(|i| i.id != item);
    }

    pub fn view(&self, registry: &SkinRegistry) -> Arc<View> {
        let rows = self
            .items
            .iter()
            .map(|item| {
                let status = registry
                    .external_status()
                    .iter()
                    .find(|s| s.item == item.id);
                let package = status.and_then(|s| s.id.clone());
                let details = self.details.get(&item.id);
                let capabilities = package
                    .as_deref()
                    .and_then(|id| registry.external_root(id).filter(|r| r.item == item.id))
                    .and_then(|r| {
                        nanatype_theme::PackageManifest::read(&nanatype_theme::DirFiles::new(
                            &r.folder,
                        ))
                        .ok()
                    })
                    .map(|m| m.capabilities);
                let kind = match capabilities {
                    Some(c) if c.contains(&PackageCapability::Skin) => Kind::Skin,
                    Some(c) if c.contains(&PackageCapability::Sounds) => Kind::SoundPack,
                    Some(_) => Kind::Other,
                    None => match details {
                        Some(d) if d.tags.iter().any(|t| t == Tag::Skins.name()) => Kind::Skin,
                        Some(d) if d.tags.iter().any(|t| t == Tag::SoundPacks.name()) => {
                            Kind::SoundPack
                        }
                        _ => Kind::Other,
                    },
                };
                let state = match item.state {
                    ItemState::Downloading => RowState::Downloading(
                        item.progress
                            .filter(|(_, total)| *total > 0)
                            .map(|(done, total)| (done * 100 / total).min(100) as u8),
                    ),
                    ItemState::Pending => RowState::Pending,
                    ItemState::NotInstalled => RowState::NotInstalled,
                    ItemState::Installed | ItemState::NeedsUpdate => {
                        match status.and_then(|s| s.problem.clone()) {
                            Some(ExternalProblem::Unreadable(why)) => RowState::Invalid(why),
                            Some(ExternalProblem::Shadowed) => {
                                RowState::Shadowed(package.clone().unwrap_or_default())
                            }
                            None if status.is_none() => RowState::Pending,
                            None => match package.as_deref().and_then(|id| registry.entry(id)) {
                                Some(entry) if !entry.loads() => RowState::Invalid(
                                    entry
                                        .errors
                                        .first()
                                        .map(ToString::to_string)
                                        .unwrap_or_default(),
                                ),
                                _ => RowState::Ready,
                            },
                        }
                    }
                };
                Row {
                    item: item.id,
                    title: details
                        .map(|d| d.title.clone())
                        .filter(|t| !t.is_empty())
                        .or_else(|| package.clone())
                        .unwrap_or_else(|| item.id.to_string()),
                    kind,
                    state,
                    package,
                    folder: item.folder.clone(),
                }
            })
            .collect();
        Arc::new(View {
            availability: self.availability.clone(),
            rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn item(id: u64, state: ItemState, folder: Option<&Path>, timestamp: u32) -> Item {
        Item {
            id,
            state,
            progress: None,
            folder: folder.map(Path::to_path_buf),
            timestamp,
        }
    }

    fn package(root: &Path, id: &str, capability: &str, file: (&str, &str)) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("package.toml"),
            format!(
                "format = 1\nschema = 3\nid = \"{id}\"\nversion = \"1.0.0\"\nmin_app = \"0.1.0\"\ncapabilities = [\"{capability}\"]\n"
            ),
        )
        .unwrap();
        fs::write(root.join(file.0), file.1).unwrap();
    }

    #[test]
    fn installed_items_feed_the_registry_and_rows_say_what_each_is() {
        let dir = tempfile::tempdir().unwrap();
        let skin = dir.path().join("1");
        let sounds = dir.path().join("2");
        package(&skin, "ws.skin", "skin", ("skin.toml", "[chrome]\n"));
        package(
            &sounds,
            "ws.sounds",
            "sounds",
            ("sounds.toml", "[sounds]\n"),
        );
        let mut workshop = Workshop::new(Availability::Ready, Vec::new());
        assert!(workshop.set_items(vec![
            item(1, ItemState::Installed, Some(&skin), 10),
            item(2, ItemState::NeedsUpdate, Some(&sounds), 10),
            item(3, ItemState::Downloading, None, 0),
        ]));
        // Progress alone is not new content.
        let mut downloading = workshop.items.clone();
        downloading[2].progress = Some((5, 10));
        assert!(!workshop.set_items(downloading));
        workshop.set_details(vec![Details {
            id: 3,
            title: "Rain".into(),
            tags: vec![Tag::SoundPacks.name().into()],
        }]);
        let roots = workshop.roots();
        assert_eq!(roots.iter().map(|r| r.item).collect::<Vec<_>>(), [1, 2]);
        let registry = SkinRegistry::with_external(None, "1.0.0", roots);
        let view = workshop.view(&registry);
        let row = |id: u64| view.rows.iter().find(|r| r.item == id).unwrap();
        assert_eq!((row(1).kind, &row(1).state), (Kind::Skin, &RowState::Ready));
        assert_eq!(row(1).package.as_deref(), Some("ws.skin"));
        assert_eq!(row(2).kind, Kind::SoundPack);
        assert_eq!(
            (row(3).kind, &row(3).state, row(3).title.as_str()),
            (Kind::SoundPack, &RowState::Downloading(Some(50)), "Rain")
        );
        assert_eq!(view.row_of("ws.skin").map(|r| r.item), Some(1));

        // An update in place means scanning again; forgetting drops the root.
        assert!(workshop.set_items(vec![
            item(1, ItemState::Installed, Some(&skin), 11),
            item(2, ItemState::NeedsUpdate, Some(&sounds), 10),
        ]));
        workshop.forget(1);
        assert_eq!(workshop.roots().len(), 1);
    }
}
