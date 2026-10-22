use proptest::prelude::*;

use crate::*;

fn bold() -> Style {
    Style {
        bold: Some(true),
        ..Style::default()
    }
}

#[test]
fn colours_round_trip_through_hex() {
    let c: Rgba8 = "#1b2f4b".parse().unwrap();
    assert_eq!(c, Rgba8::rgb(0x1b, 0x2f, 0x4b));
    assert_eq!(c.to_hex(), "#1b2f4b");
    let a: Rgba8 = "f008".parse().unwrap();
    assert_eq!(a, Rgba8::rgba(255, 0, 0, 0x88));
    assert_eq!(a.to_hex(), "#ff000088");
    assert!("#12345".parse::<Rgba8>().is_err());
    assert!("#zzzzzz".parse::<Rgba8>().is_err());
}

#[test]
fn classes_cover_cjk_latin_and_full_width_punctuation() {
    assert_eq!(classify("你"), CharClass::Cjk);
    assert_eq!(classify("か"), CharClass::Cjk);
    assert_eq!(classify("한"), CharClass::Cjk);
    assert_eq!(classify("a"), CharClass::Latin);
    assert_eq!(classify("。"), CharClass::Punct(PunctKind::Stop));
    assert_eq!(classify("，"), CharClass::Punct(PunctKind::Comma));
    assert_eq!(classify("…"), CharClass::Punct(PunctKind::Ellipsis));
    assert_eq!(classify("「"), CharClass::Punct(PunctKind::Other));
    assert_eq!(classify(" "), CharClass::Space);
    assert_eq!(classify("👍🏽"), CharClass::Latin);
}

#[test]
fn plain_text_splits_paragraphs_on_any_newline() {
    let doc = Document::plain("一\r\n二\r三\n四");
    assert_eq!(doc.paragraphs.len(), 4);
    assert_eq!(doc.plain_text(), "一\n二\n三\n四");
}

#[test]
fn normalization_merges_equal_neighbours_and_drops_empty_runs() {
    let doc = Document::from_inlines(vec![
        Inline::Text(TextRun::styled("a", bold())),
        Inline::Text(TextRun::plain("")),
        Inline::Text(TextRun::styled("b", bold())),
        Inline::Text(TextRun::plain("c")),
    ]);
    assert_eq!(doc.paragraphs[0].inlines.len(), 2);
    match &doc.paragraphs[0].inlines[0] {
        Inline::Text(run) => assert_eq!(run.text, "ab"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn units_count_graphemes_atoms_and_ruby_bases_but_not_markers() {
    let doc = Document::from_inlines(vec![
        Inline::text("e\u{301}x"),
        Inline::Marker(Marker::Wait { ms: 300 }),
        Inline::Ruby(RubyGroup {
            base: TextRun::plain("漢字"),
            annotation: "かんじ".into(),
        }),
        Inline::Atom(Atom {
            kind: AtomKind::Sticker,
            asset: "lib:cats/hi".parse().unwrap(),
            height_em: 1.5,
            alt: "[hi]".into(),
            effects: Effects::default(),
        }),
    ]);
    let units = doc.units();
    assert_eq!(units.len(), 5);
    assert_eq!(doc.unit_text(&units[0]), "e\u{301}");
    assert!(matches!(
        units[2].kind,
        UnitKind::Grapheme { ruby: true, .. }
    ));
    assert_eq!(units[4].kind, UnitKind::Atom);
    let markers = doc.markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].0, 2);
    assert_eq!(doc.plain_text(), "e\u{301}x漢字[hi]");
}

#[test]
fn blank_lines_ignore_markers_and_whitespace() {
    assert!(Document::plain("  ").is_blank());
    assert!(Document::from_inlines(vec![Inline::Marker(Marker::Page)]).is_blank());
    assert!(!Document::plain("x").is_blank());
}

#[test]
fn unit_budget_caps_long_lines() {
    let doc = Document::plain(&"a".repeat(MAX_UNITS + 50));
    assert_eq!(doc.unit_count(), MAX_UNITS);
}

#[test]
fn prefix_keeps_markers_before_the_cut() {
    let doc = Document::from_inlines(vec![
        Inline::text("ab"),
        Inline::Marker(Marker::Pause { auto_ms: None }),
        Inline::text("cd"),
    ]);
    let head = doc.prefix(2);
    assert_eq!(head.plain_text(), "ab");
    let head = doc.prefix(3);
    assert_eq!(head.plain_text(), "abc");
    assert_eq!(head.markers().len(), 1);
}

#[test]
fn suffix_starts_a_page_mid_line() {
    let doc = Document::from_inlines(vec![
        Inline::text("ab"),
        Inline::Marker(Marker::Page),
        Inline::text(
            "cd
ef",
        ),
    ]);
    assert_eq!(doc.suffix(0), doc);
    let tail = doc.suffix(2);
    assert_eq!(
        tail.plain_text(),
        "cd
ef"
    );
    assert_eq!(tail.unit_count(), doc.unit_count() - 2);
    assert_eq!(tail.markers().len(), 1, "markers at the cut stay");
    let tail = doc.suffix(3);
    assert_eq!(
        tail.plain_text(),
        "d
ef"
    );
    assert!(tail.markers().is_empty());
    // A page that begins after a hard break does not open on a blank line.
    let tail = doc.suffix(4);
    assert_eq!(tail.plain_text(), "ef");
    assert_eq!(tail.paragraphs.len(), 1);
    let tail = doc.suffix(6);
    assert_eq!(tail.unit_count(), 0);
    assert!(!tail.paragraphs.is_empty());
}

#[test]
fn suffix_cuts_ruby_bases_and_skips_atoms() {
    let doc = Document::from_inlines(vec![
        Inline::Ruby(RubyGroup {
            base: TextRun::plain("漢字"),
            annotation: "かんじ".into(),
        }),
        Inline::text("x"),
    ]);
    let tail = doc.suffix(1);
    assert_eq!(tail.plain_text(), "字x");
    assert_eq!(tail.unit_count(), 2);
}

#[test]
fn asset_refs_parse_and_print() {
    for text in [
        "blob:abc",
        "skin:stickers/a.png",
        "lib:cats/hello",
        "file:C:/x.png",
    ] {
        let asset: AssetRef = text.parse().unwrap();
        assert_eq!(asset.to_string(), text);
    }
    assert!("nope".parse::<AssetRef>().is_err());
    assert!("lib:noslash".parse::<AssetRef>().is_err());
}

#[test]
fn envelope_round_trips_and_rejects_foreign_json() {
    let doc = Document::from_inlines(vec![
        Inline::Text(TextRun {
            text: "揺れる".into(),
            style: Style {
                color: Some(Rgba8::rgb(255, 0, 0)),
                stroke: Some(Stroke {
                    color: Rgba8::WHITE,
                    width_px: 2.0,
                }),
                ..Style::default()
            },
            effects: Effects {
                continuous: vec![Continuous::new(ContinuousKind::Shake)],
                entrance: None,
                emphasis: Some(Emphasis { ms: 150 }),
            },
            pace: Pace::Scale(0.5),
        }),
        Inline::Marker(Marker::Expression {
            name: "smile".into(),
            intensity: None,
        }),
    ]);
    let json = DocumentEnvelope::to_json(&doc);
    assert_eq!(DocumentEnvelope::from_json(&json).unwrap(), doc);
    assert!(matches!(
        DocumentEnvelope::from_json(r#"{"format":"other","v":1,"doc":{}}"#),
        Err(EnvelopeError::WrongFormat(_))
    ));
    assert!(matches!(
        DocumentEnvelope::from_json(r#"{"format":"nanatype.document","v":99,"doc":{}}"#),
        Err(EnvelopeError::TooNew(99))
    ));
}

#[test]
fn style_merge_prefers_the_override() {
    let base = Style {
        color: Some(Rgba8::BLACK),
        bold: Some(true),
        ..Style::default()
    };
    let over = Style {
        color: Some(Rgba8::WHITE),
        ..Style::default()
    };
    let merged = base.merged(&over);
    assert_eq!(merged.color, Some(Rgba8::WHITE));
    assert_eq!(merged.bold, Some(true));
}

#[test]
fn effects_switch_adds_replaces_and_removes() {
    let mut effects = Effects::default();
    effects.switch(Continuous::new(ContinuousKind::Wave));
    effects.switch(Continuous::new(ContinuousKind::Shake));
    assert!(effects.has(ContinuousKind::Wave));
    assert_eq!(effects.continuous[0].kind, ContinuousKind::Shake);
    // The same kind at other values replaces it.
    let strong = Continuous {
        amp: 2.0,
        ..Continuous::new(ContinuousKind::Wave)
    };
    effects.switch(strong);
    assert_eq!(effects.continuous[1], strong);
    effects.switch(strong);
    assert!(!effects.has(ContinuousKind::Wave));
}

fn arb_inline() -> impl Strategy<Value = Inline> {
    prop_oneof![
        "[a-c你好\n\r 。]{0,6}".prop_map(|t| Inline::Text(TextRun::plain(t))),
        "[a-c你好]{0,4}".prop_map(|t| Inline::Text(TextRun::styled(t, bold()))),
        ("[漢字]{0,3}", "[かな]{0,3}").prop_map(|(b, a)| Inline::Ruby(RubyGroup {
            base: TextRun::plain(b),
            annotation: a,
        })),
        (0u32..900).prop_map(|ms| Inline::Marker(Marker::Wait { ms })),
    ]
}

proptest! {
    #[test]
    fn normalization_is_idempotent(inlines in prop::collection::vec(arb_inline(), 0..12)) {
        let once = Document::from_inlines(inlines);
        let twice = once.clone().normalized();
        prop_assert_eq!(&once, &twice);
        let json = DocumentEnvelope::to_json(&once);
        prop_assert_eq!(DocumentEnvelope::from_json(&json).unwrap(), once);
    }
}

#[test]
fn vts_markers_survive_json() {
    let doc = Document::from_inlines(vec![
        Inline::text("a"),
        Inline::Marker(Marker::Vts(VtsAction::Expression {
            file: "Smile.exp3.json".into(),
            active: false,
            fade: Some(0.5),
        })),
        Inline::Marker(Marker::Vts(VtsAction::Param {
            name: "MouthOpen".into(),
            value: 1.0,
            ms: 200,
        })),
    ]);
    let json = serde_json::to_string(&doc).unwrap();
    assert!(json.contains(r#""marker":"vts""#), "{json}");
    assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), doc);
}

#[test]
fn paragraph_attributes_survive_document_operations_and_old_json() {
    let mut doc = Document::plain("first\nsecond\n");
    for paragraph in &mut doc.paragraphs {
        paragraph.align = Some(crate::ParagraphAlign::Center);
    }
    doc.normalize();
    for candidate in [
        doc.clone(),
        doc.prefix(2),
        doc.suffix(2),
        doc.without_markers(),
    ] {
        assert!(
            candidate
                .paragraphs
                .iter()
                .all(|p| p.align == Some(crate::ParagraphAlign::Center))
        );
        let json = serde_json::to_string(&candidate).unwrap();
        assert_eq!(serde_json::from_str::<Document>(&json).unwrap(), candidate);
    }
    let old: Document = serde_json::from_str(r#"{"paragraphs":[{"inlines":[]}]}"#).unwrap();
    assert_eq!(old.paragraphs[0].align, None);
}
