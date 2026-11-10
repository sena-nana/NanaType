//! The sounds workspace: the package's audio files in the side panel, the
//! events they are bound to in the centre, and the selected file's player
//! in the inspector. Each event row picks one of the package's sounds and
//! plays it in place.
use crate::msg::{FileTarget, Msg};
use crate::session::{Action, EditorView};
use crate::ui::controls::hint;
use crate::ui::kit::{self, Type, gap};
use crate::ui::shell::{panel_header, scroll, tool};
use crate::ui::views::*;
use crate::ui::{Ui, properties, rows};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Activate, Select, SelectChanged, SelectOption, Stack, Text, TextChanged};
use nana_ui::{ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_theme::editor::{AssetKind, FileInfo, PropertyValue as Value};
use nanatype_theme::{MANIFEST, SOUNDS_MANIFEST, TomlTable};
use std::sync::Arc;

/// A new named effect's name.
const NAME_W: f32 = 140.0;

fn is_audio(file: &FileInfo) -> bool {
    file.kind == AssetKind::Audio
}

/// The file the sounds are declared in: the pack's own, else the skin's.
pub fn declared_in(view: &EditorView) -> &'static str {
    if view.documents.contains_key(SOUNDS_MANIFEST) {
        SOUNDS_MANIFEST
    } else {
        MANIFEST
    }
}

/// A TOML string literal.
fn quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn set(file: &'static str, path: &[&str], value: Option<String>) -> Msg {
    Msg::PackageEditor(Action::SetDeclaration {
        file: file.into(),
        path: path.iter().map(|p| (*p).to_owned()).collect(),
        value,
        unbind: false,
    })
}

pub fn side(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .children((
            panel_header(
                ui.now_t("pkgedit-sound-files"),
                widget(tool(ti::UPLOAD, ui.now_t("pkgedit-import")))
                    .key("package-editor-sound-import")
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::PickFile(FileTarget::ImportProjectAsset))
                    }),
            ),
            scroll(
                widget(Stack::column(0.).padding_xy(gap::TIGHT, 0.))
                    .children(super::files::file_tree(
                        ui,
                        is_audio,
                        "package-editor-sound-tree",
                    ))
                    .into_any(),
            ),
        ))
        .into_any()
}

/// Play `path` where it is named.
fn audition(ui: Ui, key: String, path: String) -> AnyView {
    widget(tool(ti::PLAYER_PLAY, ui.now_t("pkgedit-play")))
        .key(key)
        .on_cx(send(move || Action::Audition(path.clone())))
        .into_any()
}

/// The package's sounds to choose from, with `current` kept even when no
/// file answers to it: the row shows what the source says.
fn choices(ui: Ui, view: &EditorView, current: Option<&str>, none: bool) -> Vec<SelectOption> {
    let mut options = Vec::new();
    if none {
        options.push(SelectOption::new("", ui.now_t("pkgedit-sound-none")));
    }
    options.extend(
        view.files
            .iter()
            .filter(|f| is_audio(f) && f.dependency.is_none())
            .map(|f| SelectOption::new(f.path.clone(), f.path.clone())),
    );
    if let Some(current) = current.filter(|c| !c.is_empty())
        && !options.iter().any(|o| *o.value == *current)
    {
        options.push(SelectOption::new(
            current,
            format!("{current} · {}", ui.now_t("pkgedit-data-missing")),
        ));
    }
    options
}

/// One event bound to at most one sound: the choice, play, and clear.
fn slot(
    ui: Ui,
    view: &EditorView,
    file: &'static str,
    label: String,
    path: &'static [&'static str],
    current: Option<String>,
    editable: bool,
) -> AnyView {
    let key = path.join(".");
    rows::row(
        label,
        (
            widget(
                Stack::row(0.)
                    .grow(1.)
                    .shrink(1.)
                    .min_width(LengthSpec::Px(0.)),
            )
            .children(
                widget(
                    Select::new(Some(current.clone().unwrap_or_default()))
                        .options(choices(ui, view, current.as_deref(), true))
                        .size(ControlSize::Small)
                        .disabled(!editable),
                )
                .key(format!("editor-sound-slot:{key}"))
                .on_cx(move |_, e: &SelectChanged, cx| {
                    let value = (!e.value.is_empty()).then(|| quoted(&e.value));
                    cx.dispatch_program_all(set(file, path, value));
                }),
            ),
            current.map(|p| audition(ui, format!("editor-sound-play:{key}"), p)),
        ),
    )
}

/// The events and what plays on each.
pub fn center(ui: Ui) -> AnyView {
    scroll(
        dynamic(
            move || {
                ui.package_editor.with(|v| {
                    v.as_ref().map(|v| {
                        (
                            v.revision,
                            Arc::as_ptr(&v.files) as usize,
                            Arc::as_ptr(&v.properties) as usize,
                        )
                    })
                })
            },
            move |_| {
                let Some(view) = view_of(ui) else {
                    return widget(Stack::column(0.)).into_any();
                };
                let file = declared_in(&view);
                let (effective, local) = match properties::effective(&view, file) {
                    Ok(tables) => tables,
                    Err(e) => return rows::form("package-editor-sounds", hint(e)),
                };
                let editable = view.files.iter().any(|f| f.path == file && f.editable);
                let empty = TomlTable::new();
                let sounds = effective
                    .get("sounds")
                    .and_then(Value::as_table)
                    .unwrap_or(&empty);
                let text = |key: &str| {
                    sounds
                        .get(key)
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                };
                rows::form(
                    "package-editor-sounds",
                    (
                        (!editable).then(|| hint(ui.now_t("pkgedit-read-only"))),
                        typing(ui, &view, file, sounds, &local, editable),
                        rows::group(
                            ui.now_t("pkgedit-sound-lines"),
                            (),
                            (
                                slot(
                                    ui,
                                    &view,
                                    file,
                                    ui.now_t("pkgedit-sound-open"),
                                    &["sounds", "open"],
                                    text("open"),
                                    editable,
                                ),
                                slot(
                                    ui,
                                    &view,
                                    file,
                                    ui.now_t("pkgedit-sound-advance"),
                                    &["sounds", "advance"],
                                    text("advance"),
                                    editable,
                                ),
                            ),
                        ),
                        effects(ui, &view, file, sounds, editable),
                    ),
                )
            },
        )
        .into_any(),
    )
}

/// The typing sound: its variants, one of which plays per character, and
/// how often it plays.
fn typing(
    ui: Ui,
    view: &EditorView,
    file: &'static str,
    sounds: &TomlTable,
    local: &TomlTable,
    editable: bool,
) -> AnyView {
    let empty = TomlTable::new();
    let table = sounds
        .get("typing")
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let files = table
        .get("files")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let write = |files: &[String]| {
        set(
            file,
            &["sounds", "typing", "files"],
            Some(format!(
                "[{}]",
                files
                    .iter()
                    .map(|f| quoted(f))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        )
    };
    let variants = files
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let mut rest = files.clone();
            rest.remove(i);
            let remove = std::sync::Mutex::new(Some(write(&rest)));
            rows::item(
                None,
                widget(
                    Text::new(path.clone())
                        .style(Type::Small.mono())
                        .truncating(),
                ),
                (
                    audition(ui, format!("editor-sound-play:typing.{i}"), path.clone()),
                    editable.then(|| {
                        widget(tool(ti::X, ui.now_t("pkgedit-entry-remove")))
                            .key(format!("editor-sound-typing-remove:{i}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                if let Some(msg) = remove.lock().ok().and_then(|mut m| m.take()) {
                                    cx.dispatch_program_all(msg);
                                }
                            })
                    }),
                ),
            )
        })
        .collect::<Vec<_>>();
    let add = {
        let files = files.clone();
        widget(
            Select::new(None::<String>)
                .placeholder(ui.now_t("pkgedit-sound-add-variant"))
                .options(
                    choices(ui, view, None, false)
                        .into_iter()
                        .filter(|o| !files.iter().any(|f| **f == *o.value))
                        .collect::<Vec<_>>(),
                )
                .size(ControlSize::Small)
                .disabled(!editable),
        )
        .key("editor-sound-typing-add")
        .on_cx(move |_, e: &SelectChanged, cx| {
            let mut next = files.clone();
            next.push(e.value.to_string());
            cx.dispatch_program_all(set(
                file,
                &["sounds", "typing", "files"],
                Some(format!(
                    "[{}]",
                    next.iter()
                        .map(|f| quoted(f))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            ));
        })
    };
    // How it plays: the same rows the inspector draws for any value.
    let tuning = [
        ("volume", "pkgedit-sound-volume"),
        ("pitch_jitter", "pkgedit-sound-pitch-jitter"),
        ("every_n", "pkgedit-sound-every-n"),
        ("min_interval_ms", "pkgedit-sound-min-interval"),
        ("skip_punct", "pkgedit-sound-skip-punct"),
    ]
    .into_iter()
    .filter_map(|(key, caption)| {
        let value = table.get(key)?;
        let path = ["sounds", "typing", key].map(str::to_owned);
        Some(properties::property(
            ui,
            file,
            &path,
            &ui.now_t(caption),
            value,
            local,
            editable,
            rows::LABEL_W,
        ))
    })
    .collect::<Vec<_>>();
    rows::group(
        ui.now_t("pkgedit-sound-typing"),
        kit::hint_mark(ui.now_t("pkgedit-sound-typing-hint")),
        (
            widget(Stack::column(0.).gap_step(gap::step::HAIR))
                .key("editor-sound-typing")
                .children((
                    variants
                        .is_empty()
                        .then(|| hint(ui.now_t("pkgedit-sound-typing-none"))),
                    variants,
                )),
            rows::row(ui.now_t("pkgedit-sound-add-variant"), add),
            tuning,
        ),
    )
}

/// The named effects `[se=name]` plays.
fn effects(
    ui: Ui,
    view: &EditorView,
    file: &'static str,
    sounds: &TomlTable,
    editable: bool,
) -> AnyView {
    let empty = TomlTable::new();
    let table = sounds.get("se").and_then(Value::as_table).unwrap_or(&empty);
    let named = table
        .iter()
        .map(|(name, value)| {
            let current = value.as_str().unwrap_or_default().to_owned();
            let (change, remove) = (name.clone(), name.clone());
            rows::row(
                name.clone(),
                (
                    widget(
                        Stack::row(0.)
                            .grow(1.)
                            .shrink(1.)
                            .min_width(LengthSpec::Px(0.)),
                    )
                    .children(
                        widget(
                            Select::new(Some(current.clone()))
                                .options(choices(ui, view, Some(&current), false))
                                .size(ControlSize::Small)
                                .disabled(!editable),
                        )
                        .key(format!("editor-sound-se:{name}"))
                        .on_cx(move |_, e: &SelectChanged, cx| {
                            cx.dispatch_program_all(set(
                                file,
                                &["sounds", "se", &change],
                                Some(quoted(&e.value)),
                            ));
                        }),
                    ),
                    (!current.is_empty())
                        .then(|| audition(ui, format!("editor-sound-play:se.{name}"), current)),
                    editable.then(|| {
                        widget(tool(ti::X, ui.now_t("pkgedit-entry-remove")))
                            .key(format!("editor-sound-se-remove:{name}"))
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(set(
                                    file,
                                    &["sounds", "se", &remove],
                                    None,
                                ));
                            })
                    }),
                ),
            )
        })
        .collect::<Vec<_>>();
    // A new effect: its name, then the sound, which adds it.
    let name = signal(String::new());
    let taken = table.keys().cloned().collect::<Vec<_>>();
    let add = (
        widget(Stack::row(0.).width(LengthSpec::Px(NAME_W)).shrink(1.)).children(
            widget(
                kit::text_input("")
                    .placeholder(ui.now_t("pkgedit-sound-se-name"))
                    .label(ui.now_t("pkgedit-sound-se-name"))
                    .read_only(!editable)
                    .size(ControlSize::Small),
            )
            .key("editor-sound-se-name")
            .on(move |e: &TextChanged| name.set(e.value.to_string())),
        ),
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.)),
        )
        .children(
            widget(
                Select::new(None::<String>)
                    .placeholder(ui.now_t("pkgedit-sound-se-file"))
                    .options(choices(ui, view, None, false))
                    .size(ControlSize::Small),
            )
            .key("editor-sound-se-add")
            .bind(move |s: &mut Select| {
                s.disabled = !editable || name.with(|n| n.trim().is_empty());
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let id = name.get_untracked().trim().to_owned();
                let valid = !id.is_empty()
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'));
                cx.dispatch_program_all(if !valid || taken.contains(&id) {
                    Msg::Notice {
                        text: ui.now_t("pkgedit-sound-se-invalid"),
                        error: true,
                    }
                } else {
                    set(file, &["sounds", "se", &id], Some(quoted(&e.value)))
                });
            }),
        ),
    );
    rows::group(
        ui.now_t("pkgedit-sound-effects"),
        kit::hint_mark(ui.now_t("pkgedit-sound-effects-hint")),
        (
            widget(Stack::column(0.).gap_step(gap::step::TIGHT))
                .key("editor-sound-effects")
                .children(named),
            rows::row(
                ui.now_t("pkgedit-sound-se-add"),
                widget(
                    Stack::row(0.)
                        .gap_step(gap::step::TIGHT)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill),
                )
                .children(add),
            ),
        ),
    )
}

/// The selected file: its player, and the events that name it.
pub fn inspector(ui: Ui) -> AnyView {
    let body = dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref()
                    .map(|v| (v.selected.clone(), Arc::as_ptr(&v.report) as usize))
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let Some(file) = view
                .selected
                .as_ref()
                .and_then(|p| view.files.iter().find(|f| &f.path == p))
                .filter(|f| is_audio(f))
            else {
                return hint(ui.now_t("pkgedit-sound-select"));
            };
            let used = view
                .report
                .references
                .iter()
                .filter(|r| r.target == file.path)
                .map(|r| r.path.join("."))
                .collect::<Vec<_>>();
            widget(Stack::column(0.).gap_step(gap::step::CONTROL))
                .children((
                    widget(
                        Text::new(file.path.clone())
                            .style(Type::Small.mono())
                            .truncating(),
                    ),
                    crate::ui::assets::preview(ui),
                    rows::group(
                        ui.now_t("pkgedit-sound-used"),
                        (),
                        if used.is_empty() {
                            hint(ui.now_t("pkgedit-unused-asset"))
                        } else {
                            widget(readable(used.join("\n"))).into_any()
                        },
                    ),
                ))
                .into_any()
        },
    );
    widget(Stack::fill_column(0.))
        .key("package-editor-sound-inspector")
        .children((
            panel_header(ui.now_t("pkgedit-sound-audition"), ()),
            scroll(
                widget(Stack::column(0.).padding_xy(gap::CONTROL, gap::TIGHT))
                    .children(body)
                    .into_any(),
            ),
        ))
        .into_any()
}
