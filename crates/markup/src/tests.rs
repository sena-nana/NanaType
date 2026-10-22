use nanatype_model::*;
use proptest::prelude::*;

use crate::*;

fn runs(doc: &Document) -> Vec<&TextRun> {
    doc.paragraphs
        .iter()
        .flat_map(|p| &p.inlines)
        .filter_map(|i| match i {
            Inline::Text(run) => Some(run),
            _ => None,
        })
        .collect()
}

#[test]
fn styles_nest_and_close() {
    let parsed = parse("[b]粗[color=#ff0000]红[/color][/b]常");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let r = runs(&parsed.doc);
    assert_eq!(r.len(), 3);
    assert_eq!(r[0].style.bold, Some(true));
    assert_eq!(r[1].style.color, Some(Rgba8::rgb(255, 0, 0)));
    assert_eq!(r[1].style.bold, Some(true));
    assert!(r[2].style.is_plain());
}

#[test]
fn misnested_close_removes_the_matching_span_only() {
    let parsed = parse("[b][i]x[/b]y[/i]");
    assert!(parsed.diagnostics.is_empty());
    let r = runs(&parsed.doc);
    assert_eq!(r[1].text, "y");
    assert_eq!(r[1].style.italic, Some(true));
    assert_eq!(r[1].style.bold, None);
}

#[test]
fn unknown_and_bad_tags_stay_as_text() {
    let parsed = parse("a[foo]b[color=nope]c[/x]d[笑]");
    assert_eq!(parsed.doc.plain_text(), "a[foo]b[color=nope]c[/x]d[笑]");
    let kinds: Vec<_> = parsed.diagnostics.iter().map(|d| d.kind.clone()).collect();
    assert_eq!(
        kinds,
        vec![
            DiagnosticKind::UnknownTag("foo".into()),
            DiagnosticKind::BadValue {
                tag: "color".into(),
                value: "nope".into()
            },
            DiagnosticKind::UnmatchedClose("x".into()),
        ]
    );
}

#[test]
fn double_bracket_is_a_literal_bracket() {
    let parsed = parse("[[b]");
    assert_eq!(parsed.doc.plain_text(), "[b]");
    assert!(parsed.diagnostics.is_empty());
}

#[test]
fn unclosed_spans_close_at_the_end() {
    let parsed = parse("[wave]hi");
    assert_eq!(
        parsed.diagnostics[0].kind,
        DiagnosticKind::Unclosed("wave".into())
    );
    assert!(runs(&parsed.doc)[0].effects.has(ContinuousKind::Wave));
}

#[test]
fn effects_pacing_and_entrances_parse() {
    let parsed = parse("[shake amp=2][rainbow speed=0.5][pop=90][speed=40ms]x[/speed][instant]y");
    assert_eq!(
        parsed.diagnostics.len(),
        4,
        "shake, rainbow, pop and instant stay open"
    );
    let r = runs(&parsed.doc);
    let fx = &r[0].effects;
    assert_eq!(fx.continuous.len(), 2);
    assert_eq!(fx.continuous[0].kind, ContinuousKind::Shake);
    assert_eq!(fx.continuous[0].amp, 2.0);
    assert_eq!(fx.continuous[1].speed, 0.5);
    assert_eq!(
        fx.entrance,
        Some(Entrance {
            kind: EntranceKind::Pop,
            ms: 90
        })
    );
    assert_eq!(r[0].pace, Pace::FixedMs(40));
    assert_eq!(r[1].pace, Pace::Instant);
}

#[test]
fn point_tags_become_markers_and_atoms() {
    let parsed = parse(
        "a[wait=500][pause][pause auto=2000][page][se=coin vol=0.5][expr=smile][portrait=happy][screenshake=4,200][sticker=cats/hello h=1.5 alt=\"hi there\"][emote=wave]",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let inlines = &parsed.doc.paragraphs[0].inlines;
    assert_eq!(inlines[1], Inline::Marker(Marker::Wait { ms: 500 }));
    assert_eq!(
        inlines[3],
        Inline::Marker(Marker::Pause {
            auto_ms: Some(2000)
        })
    );
    assert_eq!(
        inlines[5],
        Inline::Marker(Marker::Sound {
            sound: "coin".into(),
            gain: 0.5
        })
    );
    assert_eq!(
        inlines[8],
        Inline::Marker(Marker::ScreenShake { amp: 4.0, ms: 200 })
    );
    match &inlines[9] {
        Inline::Atom(atom) => {
            assert_eq!(
                atom.asset,
                AssetRef::Library {
                    lib: "cats".into(),
                    name: "hello".into()
                }
            );
            assert_eq!(atom.height_em, 1.5);
            assert_eq!(atom.alt, "hi there");
        }
        other => panic!("{other:?}"),
    }
    match &inlines[10] {
        Inline::Atom(atom) => assert_eq!(atom.asset, AssetRef::Skin("emotes/wave".into())),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ruby_takes_the_outer_style() {
    let parsed = parse("[b][ruby=かんじ]漢字[/ruby][/b]");
    assert!(parsed.diagnostics.is_empty());
    match &parsed.doc.paragraphs[0].inlines[0] {
        Inline::Ruby(ruby) => {
            assert_eq!(ruby.base.text, "漢字");
            assert_eq!(ruby.annotation, "かんじ");
            assert_eq!(ruby.base.style.bold, Some(true));
        }
        other => panic!("{other:?}"),
    }
    let parsed = parse("[ruby=x]a[b]b[/ruby]");
    assert_eq!(
        parsed.diagnostics[0].kind,
        DiagnosticKind::IgnoredInRuby("b".into())
    );
}

#[test]
fn header_sets_line_meta_only_at_the_start() {
    let parsed = parse("[@auto=1500 after=fade delay=800 per=20]你好");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert_eq!(parsed.meta.auto_advance_ms, Some(1500));
    assert_eq!(
        parsed.meta.after,
        Some(AfterLine::Fade {
            delay_ms: 800,
            per_unit_ms: 20
        })
    );
    let late = parse("x[@auto=1]");
    assert_eq!(late.diagnostics[0].kind, DiagnosticKind::MisplacedHeader);
    let line = serialize_line(&parsed.doc, &parsed.meta);
    let again = parse(&line);
    assert_eq!(again.meta, parsed.meta);
    assert_eq!(again.doc, parsed.doc);
}

#[test]
fn newlines_split_paragraphs_with_styles_kept_open() {
    let parsed = parse("[b]一\n二[/b]");
    assert_eq!(parsed.doc.paragraphs.len(), 2);
    assert_eq!(runs(&parsed.doc)[1].style.bold, Some(true));
}

#[test]
fn serialize_is_canonical_and_shares_open_spans() {
    let parsed = parse("[b]a[/b][b][i]b[/i][/b]");
    assert_eq!(serialize(&parsed.doc), "[b]a[i]b[/i][/b]");
    let quoted = parse("[font=\"Noto Serif\"]x[/font]");
    assert_eq!(serialize(&quoted.doc), "[font=\"Noto Serif\"]x[/font]");
}

#[test]
fn templates_expand_known_keys() {
    let out = expand_template("{{ name }}说：{{x}} {{{{lit", |k| {
        (k == "name").then(|| "奈奈".to_owned())
    });
    assert_eq!(out, "奈奈说：{{x}} {{lit");
}

fn arb_style() -> impl Strategy<Value = Style> {
    (
        prop::option::of(any::<bool>()),
        prop::option::of(any::<bool>()),
        prop::option::of((any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())),
        prop::option::of(prop_oneof![
            (1u8..40).prop_map(|v| Size::Em(f32::from(v) / 8.0)),
            (8u8..90).prop_map(|v| Size::Px(f32::from(v)))
        ]),
        prop::option::of(prop_oneof![
            Just(FontSlot::Accent),
            Just(FontSlot::Family("Noto Serif SC".into()))
        ]),
    )
        .prop_map(|(bold, underline, color, size, font)| Style {
            bold,
            underline,
            color: color.map(|(r, g, b, a)| Rgba8::rgba(r, g, b, a)),
            size,
            font,
            ..Style::default()
        })
}

fn arb_effects() -> impl Strategy<Value = Effects> {
    (
        prop::collection::btree_set(0usize..ContinuousKind::ALL.len(), 0..3),
        prop::option::of((0usize..4, 1u16..900)),
        prop::option::of(0u16..900),
    )
        .prop_map(|(kinds, entrance, emphasis)| Effects {
            continuous: kinds
                .into_iter()
                .map(|k| Continuous::new(ContinuousKind::ALL[k]))
                .collect(),
            entrance: entrance.map(|(k, ms)| Entrance {
                kind: EntranceKind::ALL[k],
                ms: if k == 0 { 160 } else { ms },
            }),
            emphasis: emphasis.map(|ms| Emphasis { ms }),
        })
}

fn arb_inline() -> impl Strategy<Value = Inline> {
    prop_oneof![
        3 => ("[a-c你好\\[\\] \"\n]{1,6}", arb_style(), arb_effects(),
            prop_oneof![Just(Pace::Inherit), Just(Pace::Instant), (1u16..200).prop_map(Pace::FixedMs)])
            .prop_map(|(text, style, effects, pace)| Inline::Text(TextRun { text, style, effects, pace })),
        1 => ("[漢字]{1,3}", "[かな \\]]{1,3}").prop_map(|(b, a)| Inline::Ruby(RubyGroup {
            base: TextRun::plain(b),
            annotation: a,
        })),
        1 => prop_oneof![
            (0u32..900).prop_map(|ms| Marker::Wait { ms }),
            Just(Marker::Page),
            prop::option::of(0u32..5000).prop_map(|auto_ms| Marker::Pause { auto_ms }),
            "[a-z ]{1,5}".prop_map(|name| Marker::Expression { name, intensity: None }),
        ].prop_map(Inline::Marker),
        1 => (arb_effects(), "[a-z ]{0,4}").prop_map(|(effects, alt)| Inline::Atom(Atom {
            kind: AtomKind::Sticker,
            asset: AssetRef::Library { lib: "cats".into(), name: "hi".into() },
            height_em: 1.25,
            alt,
            effects,
        })),
        1 => (arb_effects(), "[0-9]{0,4}", r#"[a-z奈 \]"]{0,3}[a-z奈]"#, prop::bool::ANY)
            .prop_map(|(effects, uid, name, avatar)| Inline::Mention(Mention {
                user: ChatUser {
                    uid,
                    name,
                    avatar: avatar.then(|| AssetRef::Blob("bili-face-1".into())),
                },
                effects,
            })),
    ]
}

#[test]
fn mention_parses_with_uid_name_and_avatar() {
    let parsed = parse(r#"hi [at=1234 name="奈 奈" avatar=blob:bili-face-1234]!"#);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let inlines = &parsed.doc.paragraphs[0].inlines;
    assert_eq!(
        inlines[1],
        Inline::Mention(Mention::new(ChatUser {
            uid: "1234".into(),
            name: "奈 奈".into(),
            avatar: Some(AssetRef::Blob("bili-face-1234".into())),
        }))
    );
    assert_eq!(parsed.doc.unit_count(), 5);
    assert_eq!(parsed.doc.plain_text(), "hi @奈 奈!");
}

#[test]
fn mention_without_name_stays_literal() {
    let parsed = parse("[at=1]");
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(parsed.doc.plain_text(), "[at=1]");
}

proptest! {
    #[test]
    fn serialize_then_parse_is_identity(inlines in prop::collection::vec(arb_inline(), 0..10)) {
        let doc = Document::from_inlines(inlines);
        let text = serialize(&doc);
        let parsed = parse(&text);
        prop_assert!(parsed.diagnostics.is_empty(), "{text:?} {:?}", parsed.diagnostics);
        prop_assert_eq!(parsed.doc, doc, "{}", text);
    }

    #[test]
    fn parser_never_panics(text in "\\PC{0,64}") {
        let _ = parse(&text);
    }

    #[test]
    fn parser_never_panics_on_tag_soup(text in "[\\[\\]/=a-z0-9 \"@#,.\n]{0,64}") {
        let parsed = parse(&text);
        let _ = serialize(&parsed.doc);
    }
}

#[test]
fn vts_tags_parse_and_print_back() {
    let markup = "a[vts hotkey=\"Wave Hand\"]b[vts expr=Smile.exp3.json][vts expr=Smile.exp3.json active=off fade=0.3][vts param=MouthSmile value=0.8 ms=1000][vts param=EyeOpen value=0]";
    let parsed = parse(markup);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let markers: Vec<_> = parsed
        .doc
        .markers()
        .into_iter()
        .map(|(_, m)| m.clone())
        .collect();
    assert_eq!(
        markers,
        vec![
            Marker::Vts(VtsAction::Hotkey {
                hotkey: "Wave Hand".into()
            }),
            Marker::Vts(VtsAction::Expression {
                file: "Smile.exp3.json".into(),
                active: true,
                fade: None
            }),
            Marker::Vts(VtsAction::Expression {
                file: "Smile.exp3.json".into(),
                active: false,
                fade: Some(0.3)
            }),
            Marker::Vts(VtsAction::Param {
                name: "MouthSmile".into(),
                value: 0.8,
                ms: 1000
            }),
            Marker::Vts(VtsAction::Param {
                name: "EyeOpen".into(),
                value: 0.0,
                ms: 0
            }),
        ]
    );
    assert_eq!(serialize(&parsed.doc), markup);
}

#[test]
fn malformed_vts_tags_are_reported() {
    for bad in [
        "[vts]",
        "[vts=x]",
        "[vts hotkey=]",
        "[vts expr=a active=maybe]",
        "[vts param=p]",
        "[vts param=p value=x]",
    ] {
        let parsed = parse(bad);
        assert!(!parsed.diagnostics.is_empty(), "{bad}");
        assert!(parsed.doc.markers().is_empty(), "{bad}");
    }
}

#[test]
fn pulse_flicker_and_nested_continuous_effects_roundtrip() {
    let input = "[pulse amp=0.5 speed=2][flicker][wave][rainbow]脉动漢字[/rainbow][/wave][/flicker][/pulse]";
    let parsed = parse(input);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let fx = &runs(&parsed.doc)[0].effects;
    assert_eq!(fx.continuous.len(), 4);
    assert!(fx.has(ContinuousKind::Pulse) && fx.has(ContinuousKind::Flicker));
    let markup = crate::serialize(&parsed.doc);
    assert_eq!(parse(&markup).doc, parsed.doc);
}

#[test]
fn emphasis_parses_and_roundtrips() {
    let parsed =
        parse("前[em]重要[/em][em=250ms][b=0][shake][speed=0.5]事[/speed][/shake][/b][/em]后");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let r = runs(&parsed.doc);
    assert_eq!(r[0].effects.emphasis, None);
    assert_eq!(
        r[1].effects.emphasis,
        Some(Emphasis {
            ms: Emphasis::DEFAULT_MS
        })
    );
    assert_eq!(r[1].style.bold, None, "emphasis is not a bold tag");
    assert_eq!(r[2].effects.emphasis, Some(Emphasis { ms: 250 }));
    assert_eq!(r[2].style.bold, Some(false));
    assert!(r[2].effects.has(ContinuousKind::Shake));
    assert_eq!(r[2].pace, Pace::Scale(0.5));
    let markup = crate::serialize(&parsed.doc);
    assert!(markup.contains("[em]重要[/em]"), "{markup}");
    assert!(markup.contains("em=250"), "{markup}");
    assert_eq!(parse(&markup).doc, parsed.doc);
    assert!(!parse("[em=fast]x[/em]").diagnostics.is_empty());
}

#[test]
fn paragraph_alignment_empty_blocks_and_nested_resets_roundtrip() {
    let parsed = parse("[align=center]一\n二[/align]\n默认\n[align=end][/align]");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert_eq!(
        parsed
            .doc
            .paragraphs
            .iter()
            .map(|p| p.align)
            .collect::<Vec<_>>(),
        vec![
            Some(nanatype_model::ParagraphAlign::Center),
            Some(nanatype_model::ParagraphAlign::Center),
            None,
            Some(nanatype_model::ParagraphAlign::End)
        ]
    );
    assert_eq!(parse(&serialize(&parsed.doc)).doc, parsed.doc);
    let doc = parse(
        "[size=32px]a[size=inherit]b[/size]c[/size][font=accent]d[font=inherit]e[/font]f[/font]",
    )
    .doc;
    let runs = doc.paragraphs[0]
        .inlines
        .iter()
        .filter_map(|i| {
            if let nanatype_model::Inline::Text(t) = i {
                Some(t)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(runs[1].style.size, None);
    assert_eq!(runs[4].style.font, None);
    assert_eq!(parse(&serialize(&doc)).doc, doc);
    assert!(!parse("[align=bad]x[/align]").diagnostics.is_empty());
}
