//! Native file dialogs and asset import/replace/conflict-copy I/O.
use std::path::PathBuf;

use nana_ui_core::{FileDialogKind, FileDialogRequest, FileFilter};
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;
use nanatype_frame::plugin_host::background;

use crate::msg::{FileTarget, Msg};
use crate::session::Action;

use super::App;
use super::worker::EditTx;

pub(super) fn read_asset(path: PathBuf) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(nanatype_theme::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

pub(super) fn pick(app: &mut App, target: FileTarget) {
    let id = app.next_dialog;
    app.next_dialog += 1;
    app.dialogs.insert(id, target);
    let t = |k: &str| app.ui.now_t(k);
    let package = FileFilter::new("NanaType package", ["ntpkg", "zip"]);
    let request = match target {
        FileTarget::SaveConflict => {
            FileDialogRequest::new(id, FileDialogKind::SaveFile).title(t("pkgedit-save-copy"))
        }
        FileTarget::OpenDirectory => FileDialogRequest::new(id, FileDialogKind::PickFolder)
            .title(t("pkgedit-open-directory")),
        FileTarget::EditPackage => FileDialogRequest::new(id, FileDialogKind::OpenFile)
            .title(t("pkgedit-open-file"))
            .filters([package]),
        FileTarget::ExportProject => FileDialogRequest::new(id, FileDialogKind::SaveFile)
            .title(t("pkgedit-export"))
            .filters([package])
            .file_name(format!(
                "{}.ntpkg",
                app.ui
                    .proj()
                    .package_editor
                    .with_untracked(|v| v.as_ref().map(|v| v.package.clone()))
                    .unwrap_or_default()
            )),
        FileTarget::ImportProjectAsset | FileTarget::ReplaceProjectAsset => FileDialogRequest::new(
            id,
            if target == FileTarget::ImportProjectAsset {
                FileDialogKind::OpenFiles
            } else {
                FileDialogKind::OpenFile
            },
        )
        .title(t("pkgedit-import"))
        .filters([FileFilter::new(
            t("pkgedit-import"),
            nanatype_theme::editor::IMPORT_EXTENSIONS.iter().copied(),
        )]),
    };
    app.commands.push(WindowCommand::OpenFileDialog {
        id: WindowId::PRIMARY,
        request,
    });
}

pub(super) fn file_chosen(app: &mut App, request: u64, paths: Vec<PathBuf>) {
    let Some(target) = app.dialogs.remove(&request) else {
        return;
    };
    if target == FileTarget::ImportProjectAsset {
        for path in paths {
            import(app, path);
        }
        return;
    }
    let Some(path) = paths.into_iter().next() else {
        if target == FileTarget::SaveConflict {
            app.conflict_copy = None;
        }
        if target == FileTarget::ReplaceProjectAsset {
            app.replace_path = None;
        }
        return;
    };
    match target {
        FileTarget::SaveConflict => {
            if let Some((session, version, name, bytes)) = app.conflict_copy.take() {
                let source = app
                    .package_editor
                    .session
                    .as_ref()
                    .filter(|s| s.id == session && s.version == version)
                    .and_then(|s| s.directory())
                    .map(|root| root.join(&name));
                let installed = app.paths.packages();
                if source.is_none() {
                    return;
                }
                background("editor-conflict-copy", &app.ctx, move || {
                    let result = (|| {
                        use std::io::Write;
                        crate::directory::check_copy_destination(
                            &path,
                            source.as_deref(),
                            &installed,
                        )?;
                        let parent = path.parent().ok_or("missing destination directory")?;
                        let mut file =
                            tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
                        file.write_all(&bytes).map_err(|e| e.to_string())?;
                        file.as_file().sync_all().map_err(|e| e.to_string())?;
                        file.persist(path).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    Msg::PackageEditor(Action::ConflictCopied {
                        session,
                        version,
                        path: name,
                        result,
                    })
                });
            }
        }
        FileTarget::OpenDirectory | FileTarget::EditPackage => app.editor_open_archive(path),
        FileTarget::ExportProject => super::worker::export_project(app, path),
        FileTarget::ImportProjectAsset => import(app, path),
        FileTarget::ReplaceProjectAsset => {
            if let Some((session, version, target)) = app.replace_path.take() {
                let Some(s) = &app.package_editor.session else {
                    return;
                };
                if s.id != session || s.version != version {
                    return;
                }
                app.enqueue_edit(EditTx {
                    name: "editor-replace-asset",
                    prefix: String::new(),
                    rename: None,
                    refresh_asset: true,
                    apply: Box::new(move |project, _| {
                        nanatype_theme::editor::replace_asset(project, &target, read_asset(path)?)
                            .map_err(|e| e.to_string())
                    }),
                });
            }
        }
    }
}

pub(super) fn import(app: &App, path: PathBuf) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(s) = app.package_editor.session.as_ref() else {
        return;
    };
    let session = s.id;
    let folder = s.import_folder(&name);
    background("editor-import", &app.ctx, move || {
        let result = read_asset(path);
        Msg::PackageEditor(Action::Imported {
            session,
            folder,
            name,
            result,
        })
    });
}
