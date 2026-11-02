//! What of a line is read aloud.
use nanatype_model::{Document, Inline};

/// The words of `doc`, or `None` when there is nothing to say (a sticker,
/// an image, only punctuation). Ruby reads as its base, as it is shown; a
/// mention reads as the viewer's name; pictures and markers are left out.
/// Paragraphs and runs of spaces become single spaces.
pub fn speakable(doc: &Document) -> Option<String> {
    let mut out = String::new();
    for paragraph in &doc.paragraphs {
        out.push(' ');
        for inline in &paragraph.inlines {
            match inline {
                Inline::Text(run) => out.push_str(&run.text),
                Inline::Ruby(ruby) => out.push_str(&ruby.base.text),
                Inline::Mention(mention) => {
                    out.push(' ');
                    out.push_str(&mention.user.name);
                    out.push(' ');
                }
                Inline::Atom(_) => out.push(' '),
                Inline::Marker(_) => {}
            }
        }
    }
    let words: Vec<&str> = out.split_whitespace().collect();
    let text = words.join(" ");
    text.chars().any(char::is_alphanumeric).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_model::{
        AssetRef, Atom, AtomKind, ChatUser, Mention, Paragraph, RubyGroup, TextRun,
    };

    fn para(inlines: Vec<Inline>) -> Paragraph {
        Paragraph {
            inlines,
            ..Default::default()
        }
    }
    fn text(s: &str) -> Inline {
        Inline::Text(TextRun::plain(s))
    }
    fn sticker() -> Inline {
        Inline::Atom(Atom {
            kind: AtomKind::Sticker,
            asset: AssetRef::Blob("ab".into()),
            height_em: 1.0,
            alt: "[sticker]".into(),
            effects: Default::default(),
        })
    }

    #[test]
    fn plain_lines_read_as_typed() {
        assert_eq!(
            speakable(&Document::plain("你好，世界")).as_deref(),
            Some("你好，世界")
        );
    }

    #[test]
    fn markup_and_pictures_are_left_out() {
        let doc = Document {
            paragraphs: vec![
                para(vec![
                    text("Look"),
                    sticker(),
                    text("here  "),
                    Inline::Ruby(RubyGroup {
                        base: TextRun::plain("漢字"),
                        annotation: "かんじ".into(),
                    }),
                ]),
                para(vec![
                    text("hi"),
                    Inline::Mention(Mention::new(ChatUser {
                        name: "Mika".into(),
                        ..Default::default()
                    })),
                    text("!"),
                ]),
            ],
        };
        assert_eq!(speakable(&doc).as_deref(), Some("Look here 漢字 hi Mika !"));
    }

    #[test]
    fn sticker_only_and_punctuation_only_lines_are_skipped() {
        let doc = Document {
            paragraphs: vec![para(vec![sticker(), text(" "), sticker()])],
        };
        assert_eq!(speakable(&doc), None);
        assert_eq!(speakable(&Document::plain("……！？")), None);
        assert_eq!(speakable(&Document::plain("  \n ")), None);
    }
}
