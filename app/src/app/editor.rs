use super::{App, EditorCommand};
use crate::msg::{MarkupEdit, Msg};
use nanatype_core::Cmd;
use nanatype_markup::{parse, serialize_line};
use nanatype_model::Document;

impl App {
    /// The input editor: markup, tools, paste and phrases.
    pub(super) fn editor_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Markup(edit) => self.markup(edit),
            Msg::Tool(tool) => self.tool(tool),
            Msg::EditorCommand(command) => self.rich_commands.push(command.into()),
            Msg::EditorChanged(rich) => self.editor_changed(rich),
            Msg::EditorPaste => {
                if let Ok(mut clipboard) = arboard::Clipboard::new()
                    && let Ok(image) = clipboard.get_image()
                {
                    let (w, h) = (image.width as u32, image.height as u32);
                    self.update(Msg::PasteImage {
                        width: w,
                        height: h,
                        rgba: image.bytes.into_owned(),
                    });
                }
            }
            Msg::ToggleSource => {
                let on = !self.ui.source_mode.get_untracked();
                self.ui.source_mode.set(on);
                self.focus_editor = true;
            }
            Msg::EditorContext { .. } | Msg::EditorAction { .. } => {}
            Msg::InsertPhrase(i) => {
                if let Some(p) = self.settings.assets.phrases.get(i).cloned() {
                    let speaker = self.ui.speaker.get_untracked().name.clone();
                    let expanded = nanatype_markup::expand_template(&p.markup, |k| match k {
                        "speaker" | "name" => Some(speaker.clone()),
                        "time" => Some(crate::ui::history::local_hm(nanatype_model::unix_millis())),
                        _ => None,
                    });
                    if self.synchronizing()
                        && nanatype_model::outgoing::image_count(&parse(&expanded).doc) > 0
                    {
                        self.notice(self.ui.now_t("bili-images-unavailable"), true);
                    } else if p.send {
                        self.run(Cmd::SendMarkup(expanded));
                    } else if self.ui.source_mode.get_untracked() {
                        self.markup(MarkupEdit::Insert(expanded));
                    } else {
                        let parsed = parse(&expanded);
                        // A phrase starting an empty draft can also set its line
                        // header, as inserting the markup source did before.
                        if self.core.draft().is_blank() && parsed.meta != Default::default() {
                            self.set_source(serialize_line(&Document::new(), &parsed.meta), true);
                        }
                        let fragment = self.editor_rich(&parsed.doc);
                        self.rich_commands.push(EditorCommand::Insert(fragment));
                        self.focus_editor = true;
                    }
                }
            }
            Msg::PasteImage {
                width,
                height,
                rgba,
            } if self.synchronizing() => {
                let _ = (width, height, rgba);
                self.notice(self.ui.now_t("bili-images-unavailable"), true);
            }
            Msg::PasteImage {
                width,
                height,
                rgba,
            } => match self.assets.store_rgba(width, height, &rgba) {
                Ok(asset) => self.insert_asset(asset),
                Err(e) => {
                    let text = self.ui.tr("err-image", &[("message", e.as_str().into())]);
                    self.notice(text, true);
                }
            },
            _ => unreachable!("not a editor message"),
        }
    }
}
