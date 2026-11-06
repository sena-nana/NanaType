//! What of a line is read aloud.
use nanatype_model::{Document, Inline, TextRun};
use unicode_segmentation::UnicodeSegmentation;

/// The words of `doc`, or `None` when there is nothing to say (a sticker,
/// an image, only punctuation). Ruby reads as its base, as it is shown; a
/// mention reads as the viewer's name; pictures and markers are left out.
/// Paragraphs and runs of spaces become single spaces.
pub fn speakable(doc: &Document) -> Option<String> {
    speakable_units(doc).map(|(text, _)| text)
}

/// [`speakable`], with the reveal unit of `doc` each byte of the text comes
/// from (a space between words takes the unit before it), so a spoken word
/// can be matched to the units on screen.
pub fn speakable_units(doc: &Document) -> Option<(String, Vec<u32>)> {
    let mut text = String::new();
    let mut units: Vec<u32> = Vec::new();
    // A space is only written once a word follows it.
    let mut space;
    let mut unit = 0u32;
    let mut push = |c: char, unit: u32, text: &mut String, space: &mut bool| {
        if c.is_whitespace() {
            *space = !text.is_empty();
            return;
        }
        if std::mem::take(space) {
            text.push(' ');
            units.push(units.last().copied().unwrap_or(unit));
        }
        text.push(c);
        units.extend(std::iter::repeat_n(unit, c.len_utf8()));
    };
    for paragraph in &doc.paragraphs {
        space = !text.is_empty();
        for inline in &paragraph.inlines {
            match inline {
                Inline::Text(TextRun { text: run, .. })
                | Inline::Ruby(nanatype_model::RubyGroup {
                    base: TextRun { text: run, .. },
                    ..
                }) => {
                    for grapheme in run.graphemes(true) {
                        for c in grapheme.chars() {
                            push(c, unit, &mut text, &mut space);
                        }
                        unit += 1;
                    }
                }
                Inline::Mention(mention) => {
                    space = !text.is_empty();
                    for c in mention.user.name.chars() {
                        push(c, unit, &mut text, &mut space);
                    }
                    space = !text.is_empty();
                    unit += 1;
                }
                Inline::Atom(_) => {
                    space = !text.is_empty();
                    unit += 1;
                }
                Inline::Marker(_) => {}
            }
        }
    }
    text.chars()
        .any(char::is_alphanumeric)
        .then_some((text, units))
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
    fn each_spoken_byte_knows_its_unit() {
        let doc = Document {
            paragraphs: vec![
                para(vec![text("a 漢"), sticker(), text("b")]),
                para(vec![text("cd")]),
            ],
        };
        // Units: a, space, 漢, sticker, b, c, d.
        let (text, units) = speakable_units(&doc).unwrap();
        assert_eq!(text, "a 漢 b cd");
        assert_eq!(units, [0, 0, 2, 2, 2, 2, 4, 4, 5, 6]);
        assert_eq!(units.len(), text.len());
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
