//! The installed packages as both products list them.

use nanatype_theme::{PackageManifest, SkinOrigin, SkinRegistry};

#[derive(Debug, Clone, PartialEq)]
pub struct PackageRow {
    pub manifest: PackageManifest,
    pub origin: SkinOrigin,
    pub integrity: Result<Option<nanatype_theme::SealStatus>, String>,
}

/// Package rows with their signature status. Hashes every package's files:
/// call it off the UI thread.
pub fn package_rows(registry: &SkinRegistry) -> Vec<PackageRow> {
    registry
        .packages()
        .into_iter()
        .map(|(manifest, origin)| {
            let integrity = registry
                .package_files(&manifest.meta.id)
                .and_then(|files| nanatype_theme::verify_integrity(&*files))
                .map_err(|e| e.to_string());
            PackageRow {
                manifest,
                origin,
                integrity,
            }
        })
        .collect()
}
