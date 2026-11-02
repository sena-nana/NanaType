//! General's profiles and backup sections, and the import confirmation.
use super::*;
use crate::msg::{BackupMsg, ProfileMsg};
use nana_ui::runtime::{ConfirmDialog, ConfirmIntent, DialogCloseRequested};
use nanatype_settings::ExportOptions;

/// The profiles: each one's name, switch, save, duplicate and delete; then
/// a name and "Save as profile". Adding, removing or switching rebuilds the
/// list; typing a name does not.
pub(super) fn profiles(ui: Ui) -> Vec<Row> {
    let list = Row::inset(dynamic(
        move || {
            let ids: Vec<String> = ui
                .backup
                .with(|b| b.profiles.iter().map(|(id, _)| id.clone()).collect());
            (ids, ui.settings.with(|s| s.general.profile.clone()))
        },
        move |(ids, active)| {
            let rows: Vec<Row> = ids
                .iter()
                .map(|id| profile_row(ui, id.clone(), active.as_ref() == Some(id)))
                .collect();
            Row::inset(widget(Stack::column(0.0)).children(divided(rows)))
        },
    ));
    let name = signal(String::new());
    let field = widget(Stack::row(0.0).width(LengthSpec::Px(field::MD))).children(
        widget(
            crate::ui::kit::text_input(String::new())
                .placeholder(ui.now_t("profile-name-placeholder"))
                .size(ControlSize::Small),
        )
        .on(move |e: &TextChanged| name.set(e.value.to_string())),
    );
    let save = button(ui, "profile-save-as", move || {
        Msg::Profile(ProfileMsg::SaveAs(name.get_untracked()))
    });
    vec![
        list,
        row(
            ui,
            "profile-new",
            Some("profile-hint"),
            controls(vec![field.into_any(), save]),
        ),
    ]
}

fn profile_row(ui: Ui, id: String, active: bool) -> Row {
    let name = ui.backup.with_untracked(|b| {
        b.profiles
            .iter()
            .find(|(p, _)| *p == id)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    });
    let rename = {
        let id = id.clone();
        widget(Stack::row(0.0).width(LengthSpec::Px(field::MD))).children(
            widget(crate::ui::kit::text_input(name).size(ControlSize::Small)).on_cx(
                move |_, e: &TextChanged, cx| {
                    cx.dispatch_program_all(Msg::Profile(ProfileMsg::Rename {
                        id: id.clone(),
                        name: e.value.to_string(),
                    }))
                },
            ),
        )
    };
    let switch = if active {
        widget(
            Text::new(ui.now_t("profile-active"))
                .style(Type::Small.style())
                .color(SemanticColorRole::Muted),
        )
        .into_any()
    } else {
        let id = id.clone();
        button(ui, "profile-switch", move || {
            Msg::Profile(ProfileMsg::Switch(id.clone()))
        })
    };
    let update = {
        let id = id.clone();
        icon_action(ui, "profile-update", ti::DEVICE_FLOPPY, move || {
            Msg::Profile(ProfileMsg::Update(id.clone()))
        })
    };
    let duplicate = {
        let id = id.clone();
        icon_action(ui, "profile-duplicate", ti::COPY, move || {
            Msg::Profile(ProfileMsg::Duplicate(id.clone()))
        })
    };
    let delete = confirm(
        *ui,
        ui.now_t("profile-delete-confirm"),
        ui.now_t("assets-remove"),
        |armed| remove_button(ui, armed),
        move || Msg::Profile(ProfileMsg::Delete(id.clone())),
    );
    crate::ui::kit::line(rename, controls(vec![switch, update, duplicate, delete]))
}

/// Export with its options, import, the backup and reset.
pub(super) fn backup(ui: Ui) -> Vec<(Row, Shown)> {
    type Pick = (
        &'static str,
        fn(&ExportOptions) -> bool,
        fn(&mut ExportOptions),
    );
    let options: [Pick; 3] = [
        (
            "backup-include-integrations",
            |o| o.integrations,
            |o| o.integrations = !o.integrations,
        ),
        (
            "backup-include-avatars",
            |o| o.avatars,
            |o| o.avatars = !o.avatars,
        ),
        (
            "backup-include-positions",
            |o| o.window_positions,
            |o| o.window_positions = !o.window_positions,
        ),
    ];
    let chips: Vec<AnyView> = options
        .into_iter()
        .map(|(key, get, flip)| {
            widget(
                Chip::new(ui.now_t(key))
                    .selected(get(&ui.backup.get_untracked().options))
                    .size(ControlSize::Small),
            )
            .bind(move |c: &mut Chip| c.selected = ui.backup.with(|b| get(&b.options)))
            .on(move |_: &Activate| {
                let mut view = (*ui.backup.get_untracked()).clone();
                flip(&mut view.options);
                ui.backup.set(Arc::new(view));
            })
            .into_any()
        })
        .collect();
    let mut export = chips;
    export.push(primary(ui, "backup-export-button", || {
        Msg::Backup(BackupMsg::Export)
    }));
    vec![
        (
            row(
                ui,
                "backup-export",
                Some("backup-export-hint"),
                controls(export),
            ),
            None,
        ),
        (
            row(
                ui,
                "backup-import",
                Some("backup-import-hint"),
                button(ui, "backup-import-button", || {
                    Msg::Backup(BackupMsg::Import)
                }),
            ),
            None,
        ),
        shown_if(
            row(
                ui,
                "backup-restore",
                Some("backup-restore-hint"),
                button(ui, "backup-restore-button", || {
                    Msg::Backup(BackupMsg::Restore)
                }),
            ),
            move || ui.backup.with(|b| b.has_previous),
        ),
        (
            row(
                ui,
                "backup-reset",
                Some("backup-reset-hint"),
                confirm(
                    *ui,
                    ui.now_t("backup-reset-confirm"),
                    ui.now_t("backup-reset-button"),
                    |armed| {
                        widget(
                            Button::new(ui.now_t("backup-reset-button")).size(ControlSize::Small),
                        )
                        .on(move |_: &Activate| armed.set(true))
                        .into_any()
                    },
                    || Msg::Backup(BackupMsg::Reset),
                ),
            ),
            None,
        ),
    ]
}

/// What a checked import will change, with Import and Cancel.
pub fn import_dialog(ui: Ui) -> AnyView {
    // It opens on Import: Enter imports (a backup is kept), Esc cancels.
    let import = nana_ui::runtime::view::entity_ref::<Button>();
    widget(
        ConfirmDialog::new(ui.now_t("backup-import-title"), String::new())
            .cancel_label(ui.now_t("common-cancel")),
    )
    .confirm(
        widget(Button::new(ui.now_t("backup-import-button")).kind(ButtonKind::Primary))
            .entity_ref(import),
    )
    .initial_focus_on(import)
    .bind(move |dialog: &mut ConfirmDialog| {
        if let Some(p) = &ui.backup.get().pending {
            dialog.message = crate::app::backup::summary(ui, p).into();
        }
    })
    .open(move || ui.backup.with(|b| b.pending.is_some()))
    .on_cx(|_, intent: &ConfirmIntent, cx| {
        cx.dispatch_program_all(Msg::Backup(BackupMsg::Confirm(matches!(
            intent,
            ConfirmIntent::Confirm { .. }
        ))))
    })
    .on_cx(|_, _: &DialogCloseRequested, cx| {
        cx.dispatch_program_all(Msg::Backup(BackupMsg::Confirm(false)))
    })
    .into_any()
}
