use super::{App, EditorCommand};
use crate::completion;
use crate::msg::{Complete, EditorMsg, MarkupEdit, Tool};
use nana_ui::runtime::RichEditCommand;
use nanatype_core::Cmd;
use nanatype_markup::{parse, serialize_line};
use nanatype_model::{Document, Inline};

impl App {
    /// The input editor: markup, tools, paste and phrases.
    pub(super) fn editor_msg(&mut self, msg: EditorMsg) {
        if matches!(
            &msg,
            EditorMsg::Command(_)
                | EditorMsg::Changed(_)
                | EditorMsg::Paste
                | EditorMsg::InsertPhrase(_)
                | EditorMsg::InsertNewline
                | EditorMsg::PasteImage { .. }
                | EditorMsg::Complete(Complete::Accept(_))
        ) {
            self.draft.bump_version();
            self.speech_user_edit();
        }
        // Moving through the list must not rebuild it.
        let refresh = !matches!(msg, EditorMsg::Complete(_));
        self.editor_edit(msg);
        if refresh {
            self.refresh_completion();
        }
    }

    fn editor_edit(&mut self, msg: EditorMsg) {
        match msg {
            EditorMsg::Complete(action) => self.complete(action),
            EditorMsg::Selection(selection) => {
                self.draft.completer.caret =
                    (selection.start == selection.end).then_some(selection.end);
            }
            EditorMsg::Markup(edit) => self.markup(edit),
            EditorMsg::Tool(tool) => self.tool(tool),
            EditorMsg::Command(command) => self.draft.rich_commands.push(command.into()),
            EditorMsg::Changed(rich) => self.editor_changed(rich),
            EditorMsg::Paste => {
                if let Some((width, height, rgba)) = self.draft.clipboard_image() {
                    self.paste_image(width, height, rgba);
                }
            }
            EditorMsg::ToggleSource => {
                let on = !self.ui.source_mode.get_untracked();
                self.ui.source_mode.set(on);
                self.draft.focus_editor = true;
            }
            EditorMsg::InsertPhrase(i) => {
                if let Some(p) = self.settings.assets.phrases.get(i).cloned() {
                    let speaker = self.ui.speaker.get_untracked().name.clone();
                    let expanded = nanatype_markup::expand_template(&p.markup, |k| match k {
                        "speaker" | "name" => Some(speaker.clone()),
                        "time" => Some(crate::ui::history::local_hm(nanatype_model::unix_millis())),
                        _ => None,
                    });
                    if self.draft.is_plain_text_only()
                        && nanatype_model::outgoing::image_count(&parse(&expanded).doc) > 0
                    {
                        self.notice(self.ui.now_t("chat-images-unavailable"), true);
                    } else if p.send {
                        self.run(Cmd::SendMarkup(expanded));
                    } else if self.ui.source_mode.get_untracked() {
                        self.markup(MarkupEdit::Insert(expanded));
                    } else {
                        let parsed = parse(&expanded);
                        // A phrase starting an empty draft can also set its line
                        // header, as inserting the markup source did before.
                        if self.session.core.draft().is_blank() && parsed.meta != Default::default()
                        {
                            self.set_source(serialize_line(&Document::new(), &parsed.meta), true);
                        }
                        let fragment = self.editor_rich(&parsed.doc);
                        self.draft
                            .rich_commands
                            .push(EditorCommand::Insert(fragment));
                        self.draft.focus_editor = true;
                    }
                }
            }
            EditorMsg::InsertNewline => {
                if self.ui.source_mode.get_untracked() {
                    self.markup(MarkupEdit::Insert("\n".into()));
                } else {
                    self.draft
                        .rich_commands
                        .push(nana_ui::runtime::RichEditCommand::InsertText("\n".into()).into());
                }
            }
            EditorMsg::PasteImage {
                width,
                height,
                rgba,
            } => self.paste_image(width, height, rgba),
        }
    }

    fn paste_image(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        if !self.draft.allow_new_image() {
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        match self.media.assets.store_rgba(width, height, &rgba) {
            Ok(asset) => self.insert_asset(asset),
            Err(e) => {
                let text = self.ui.tr("err-image", &[("message", e.as_str().into())]);
                self.notice(text, true);
            }
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
            self.draft
                .rich_commands
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
            let caret = self.draft.completer.caret;
            self.ui
                .editor_value
                .with_untracked(|v| caret.and_then(|c| completion::token(v.text(), c)))
        };
        let start = token.as_ref().map(|t| t.range.start);
        if self.draft.completer.dismissed.is_some() && self.draft.completer.dismissed != start {
            self.draft.completer.dismissed = None;
        }
        let open = token
            .filter(|_| self.draft.completer.dismissed.is_none())
            .and_then(|token| {
                let locale = self.ui.locale.get_untracked();
                let items: std::sync::Arc<[completion::Item]> = match token.trigger {
                    completion::Trigger::Mention => completion::users(
                        self.session
                            .chat
                            .iter()
                            .rev()
                            .map(|m| (m.platform, &m.user)),
                        &token.needle,
                        locale,
                    ),
                    completion::Trigger::Command => {
                        let hotkeys: Vec<String> = self.ui.links.with_untracked(|s| {
                            s.vts_hotkeys.iter().map(|(_, n)| n.clone()).collect()
                        });
                        let names = completion::Names {
                            vts_hotkeys: &hotkeys,
                        };
                        completion::commands(&self.appearance.skin, names, &token.needle, locale)
                    }
                }
                .into();
                if items.is_empty() {
                    return None;
                }
                // The choice holds while the rows stay the same.
                let (selected, first) = self
                    .draft
                    .completer
                    .open
                    .as_ref()
                    .filter(|c| c.token.range.start == token.range.start && c.items == items)
                    .map_or((0, 0), |c| (c.selected, c.first));
                Some(completion::Completion {
                    token,
                    items,
                    selected,
                    first,
                })
            });
        self.draft.completer.open = open;
        self.publish_completion();
    }

    fn publish_completion(&self) {
        let view = self.draft.completer.view();
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
        match &self.draft.completer.open {
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
                if let Some(open) = &mut self.draft.completer.open {
                    open.move_by(delta);
                }
            }
            Complete::Dismiss => {
                if let Some(open) = self.draft.completer.open.take() {
                    self.draft.completer.dismissed = Some(open.token.range.start);
                }
            }
            Complete::Accept(row) => {
                let Some(open) = self.draft.completer.open.take() else {
                    return;
                };
                let Some(item) = open.items.get(row.unwrap_or(open.selected)).cloned() else {
                    return;
                };
                let range = open.token.range;
                if !matches!(item.insert, completion::Insert::Text(_)) {
                    // The rich editor reports the new text later; the list
                    // must not reopen on the token it just replaced.
                    self.draft.completer.dismissed = Some(range.start);
                }
                if self.ui.source_mode.get_untracked() {
                    let mut src = self.ui.source.get_untracked();
                    if src.text.get(range.clone()).is_none() {
                        return;
                    }
                    src.selection = (range.start, range.end);
                    self.ui.source.set(src);
                } else {
                    self.draft
                        .rich_commands
                        .push(RichEditCommand::Select(range).into());
                }
                match item.insert {
                    completion::Insert::Inline(inline) => self.insert_inline(inline),
                    // A command's name: the list goes on to its arguments.
                    completion::Insert::Text(text) => self.replace_token(text),
                    // Gone the token, the effect is set where it was.
                    completion::Insert::Tool(tool) => {
                        self.draft.focus_editor = true;
                        if self.ui.source_mode.get_untracked() {
                            self.markup(MarkupEdit::Insert(String::new()));
                        } else {
                            self.draft.rich_commands.push(EditorCommand::Delete);
                        }
                        self.tool(tool);
                    }
                }
            }
        }
        self.publish_completion();
    }

    /// Put `text` in place of the selected token.
    fn replace_token(&mut self, text: String) {
        self.draft.focus_editor = true;
        if self.ui.source_mode.get_untracked() {
            self.markup(MarkupEdit::Insert(text));
        } else {
            self.draft
                .rich_commands
                .push(RichEditCommand::InsertText(text).into());
        }
    }
}
