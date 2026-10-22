use super::{App, EditorCommand};
use crate::completion;
use crate::msg::{Complete, MarkupEdit, Msg, Tool};
use nana_ui::runtime::RichEditCommand;
use nanatype_core::Cmd;
use nanatype_markup::{parse, serialize_line};
use nanatype_model::{Document, Inline};

impl App {
    /// The input editor: markup, tools, paste and phrases.
    pub(super) fn editor_msg(&mut self, msg: Msg) {
        if let Msg::Complete(action) = msg {
            self.complete(action);
            return;
        }
        self.editor_edit(msg);
        self.refresh_completion();
    }

    fn editor_edit(&mut self, msg: Msg) {
        match msg {
            Msg::EditorSelection(selection) => {
                self.completer.caret = (selection.start == selection.end).then_some(selection.end);
            }
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
            Msg::InsertNewline => {
                if self.ui.source_mode.get_untracked() {
                    self.markup(MarkupEdit::Insert("\n".into()));
                } else {
                    self.rich_commands
                        .push(nana_ui::runtime::RichEditCommand::InsertText("\n".into()).into());
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

impl App {
    /// Insert a mention or marker at the caret; a mention is followed by a
    /// space, so typing goes on as text.
    pub(super) fn insert_inline(&mut self, inline: Inline) {
        let mention = matches!(inline, Inline::Mention(_));
        self.tool(Tool::Insert(inline));
        if !mention {
            return;
        }
        if self.ui.source_mode.get_untracked() {
            self.markup(MarkupEdit::Insert(" ".into()));
        } else {
            self.rich_commands
                .push(RichEditCommand::InsertText(" ".into()).into());
        }
    }

    /// Open, update or close the `@` / `/` list for the text before the caret.
    pub(super) fn refresh_completion(&mut self) {
        let token = if self.ui.source_mode.get_untracked() {
            self.ui.source.with_untracked(|s| {
                let (a, b) = s.selection;
                (a == b).then(|| completion::token(&s.text, b)).flatten()
            })
        } else {
            let caret = self.completer.caret;
            self.ui
                .editor_value
                .with_untracked(|v| caret.and_then(|c| completion::token(v.text(), c)))
        };
        let start = token.as_ref().map(|t| t.range.start);
        if self.completer.dismissed.is_some() && self.completer.dismissed != start {
            self.completer.dismissed = None;
        }
        let open = token
            .filter(|_| self.completer.dismissed.is_none())
            .and_then(|token| {
                let locale = self.ui.locale.get_untracked();
                let items: std::sync::Arc<[completion::Item]> = match token.trigger {
                    completion::Trigger::Mention => completion::users(
                        self.chat.iter().rev().map(|m| (m.platform, &m.user)),
                        &token.needle,
                        locale,
                    ),
                    completion::Trigger::Command => {
                        completion::commands(&self.skin, &token.needle, locale)
                    }
                }
                .into();
                if items.is_empty() {
                    return None;
                }
                // The choice holds while the rows stay the same.
                let selected = self
                    .completer
                    .open
                    .as_ref()
                    .filter(|c| c.token.range.start == token.range.start && c.items == items)
                    .map_or(0, |c| c.selected);
                Some(completion::Completion {
                    token,
                    items,
                    selected,
                })
            });
        self.completer.open = open;
        self.publish_completion();
    }

    fn publish_completion(&self) {
        let view = self.completer.view();
        if self
            .ui
            .completion
            .with_untracked(|v| v.as_deref() != view.as_deref())
        {
            self.ui.completion.set(view);
        }
        let source = self.source_completions();
        if self.ui.source_completions.with_untracked(|c| *c != source) {
            self.ui.source_completions.set(source);
        }
    }

    fn source_completions(&self) -> std::sync::Arc<[nana_ui::runtime::TextCompletion]> {
        match &self.completer.open {
            Some(open) if self.ui.source_mode.get_untracked() => self
                .ui
                .source
                .with_untracked(|src| open.text_completions(&src.text)),
            _ => std::sync::Arc::from([]),
        }
    }

    fn complete(&mut self, action: Complete) {
        match action {
            Complete::Move(delta) => {
                if let Some(open) = &mut self.completer.open {
                    open.move_by(delta);
                }
            }
            Complete::Dismiss => {
                if let Some(open) = self.completer.open.take() {
                    self.completer.dismissed = Some(open.token.range.start);
                }
            }
            Complete::Accept(row) => {
                let Some(open) = self.completer.open.take() else {
                    return;
                };
                let Some(item) = open.items.get(row.unwrap_or(open.selected)).cloned() else {
                    return;
                };
                let range = open.token.range;
                if matches!(item.insert, completion::Insert::Inline(_)) {
                    // The rich editor reports the new text later; the list
                    // must not reopen on the token it just replaced.
                    self.completer.dismissed = Some(range.start);
                }
                if self.ui.source_mode.get_untracked() {
                    let mut src = self.ui.source.get_untracked();
                    if src.text.get(range.clone()).is_none() {
                        return;
                    }
                    src.selection = (range.start, range.end);
                    self.ui.source.set(src);
                } else {
                    self.rich_commands
                        .push(RichEditCommand::Select(range).into());
                }
                match item.insert {
                    completion::Insert::Inline(inline) => self.insert_inline(inline),
                    // A command's name: the list goes on to its arguments.
                    completion::Insert::Text(text) => {
                        self.focus_editor = true;
                        if self.ui.source_mode.get_untracked() {
                            self.markup(MarkupEdit::Insert(text));
                        } else {
                            self.rich_commands
                                .push(RichEditCommand::InsertText(text).into());
                        }
                    }
                }
            }
        }
        self.publish_completion();
    }
}
