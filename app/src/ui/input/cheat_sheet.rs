use super::panels::caption;
use super::*;

/// The kind's stripe on a marker chip in the cheat sheet.
const CHIP_STRIPE: f32 = 3.0;
/// The cheat sheet's lead column: a marker chip, or a chord of keycaps.
const SHEET_MARKERS_W: f32 = 116.0;
const SHEET_KEYS_W: f32 = 136.0;

/// A marker as the editor shows it, for the cheat sheet: the kind's stripe
/// and glyph, then its markup, on the editor's chip fill.
pub(super) fn marker_chip(
    look: &MarkerLook,
    kind: ChipKind,
    icon: nana_ui::runtime::Icon,
    markup: &str,
) -> AnyView {
    let ink = c(look.text);
    let mut glyph = IconGlyph::new(icon).size(icon::SM).decorative();
    glyph.style.foreground = None;
    Arc::make_mut(&mut glyph.style.layout).color = Some(ink);
    let mut st = Type::Overline.style();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.color = Some(ink);
        l.white_space_nowrap = true;
    }
    let fill = c(look.fill);
    let hue = c(kind.hue());
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .radius(nana_ui_core::RadiusTier::Xs)
            .with_layout(move |l| {
                l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
                l.flex_shrink = Some(0.0);
                l.background = Some(fill);
                l.padding_right = Some(LengthSpec::Px(inset::CHIP_X));
                l.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                l.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .children((
        widget(Stack::row(0.0).with_layout(move |l| {
            l.width = Some(LengthSpec::Px(CHIP_STRIPE));
            l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
            l.flex_shrink = Some(0.0);
            l.background = Some(hue);
        })),
        widget(glyph),
        widget(Text::new(markup).decorative().style(st)),
    ))
    .into_any()
}

/// One line of the cheat sheet: a fixed-width lead (a chip or keycaps) and
/// what it does.
fn sheet_row(lead: AnyView, lead_w: f32, meaning: String) -> AnyView {
    let mut st = Type::Caption.style();
    Arc::make_mut(&mut st.layout).white_space_nowrap = true;
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .with_layout(|l| {
                l.height = Some(LengthSpec::Px(crate::ui::kit::keycap::H));
            }),
    )
    .children((
        widget(
            Stack::row(0.0)
                .align(AlignSpec::Center)
                .with_layout(move |l| {
                    l.width = Some(LengthSpec::Px(lead_w));
                    l.min_width = Some(LengthSpec::Px(lead_w));
                    l.flex_shrink = Some(0.0);
                }),
        )
        .children(lead),
        widget(Text::new(meaning).style(st).color(SemanticColorRole::Muted)),
    ))
    .into_any()
}

/// The `?` sheet: the markers the editor shows as chips with what each does,
/// and the input window's own keys.
pub fn cheat_sheet(ui: Ui) -> AnyView {
    let look = super::super::rich::MentionLook::for_ui(ui.chrome_untracked()).markers;
    let t = |k: &str| ui.now_t(k);
    let markers: Vec<AnyView> = [
        (ChipKind::Wait, ti::CLOCK, "[wait=500]", "cheat-wait"),
        (
            ChipKind::Continue,
            ti::PLAYER_PLAY,
            "[pause]",
            "cheat-pause",
        ),
        (ChipKind::Page, ti::PAGE_BREAK, "[page]", "cheat-page"),
        (
            ChipKind::Expression,
            ti::MOOD_SMILE,
            "[expr=…]",
            "cheat-expr",
        ),
        (ChipKind::Sound, ti::MUSIC, "[se=…]", "cheat-se"),
        (
            ChipKind::Picture,
            ti::STICKER,
            "[sticker=…]",
            "cheat-sticker",
        ),
        (ChipKind::Mention, ti::AT, "[at=…]", "cheat-at"),
    ]
    .into_iter()
    .map(|(kind, icon, markup, key)| {
        sheet_row(
            marker_chip(&look, kind, icon, markup),
            SHEET_MARKERS_W,
            t(key),
        )
    })
    .collect();
    // The keys as bound in Settings › Hotkeys; unbound ones are left out.
    let input = ui.settings.with_untracked(|s| s.hotkeys.input.clone());
    let bound = |k: InputKey| k.get(&input).map(|h| h.label(super::super::META_LABEL));
    let row = |k: InputKey, meaning: &str| {
        bound(k).map(|h| {
            sheet_row(
                super::super::kit::keycap::chord(&h),
                SHEET_KEYS_W,
                t(meaning),
            )
        })
    };
    // Two keys that step one way and the other: "A / B".
    let pair = |prev: InputKey, next: InputKey, meaning: &str| {
        match (bound(prev), bound(next)) {
            (Some(prev), Some(next)) => Some(
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::TIGHT)
                        .align(AlignSpec::Center),
                )
                .children((
                    super::super::kit::keycap::chord(&prev),
                    widget(
                        Text::new("/")
                            .decorative()
                            .style(Type::Small.style())
                            .color(SemanticColorRole::Muted),
                    ),
                    super::super::kit::keycap::chord(&next),
                ))
                .into_any(),
            ),
            (one, other) => one.or(other).map(|h| super::super::kit::keycap::chord(&h)),
        }
        .map(|lead| sheet_row(lead, SHEET_KEYS_W, t(meaning)))
    };
    let keys: Vec<AnyView> = [
        row(InputKey::Send, "tb-send"),
        row(InputKey::Newline, "cheat-newline"),
        row(InputKey::Bold, "tb-bold"),
        row(InputKey::Italic, "tb-italic"),
        row(InputKey::Underline, "tb-underline"),
        pair(InputKey::RecallPrev, InputKey::RecallNext, "cheat-recall"),
        pair(InputKey::ChatPrev, InputKey::ChatNext, "cheat-chat-select"),
        row(InputKey::Reply, "hotkeys-reply"),
        row(InputKey::Hide, "cheat-hide"),
        // The `@` / `/` list's own key; not rebindable.
        Some(sheet_row(
            super::super::kit::keycap::chord("Tab"),
            SHEET_KEYS_W,
            t("cheat-complete"),
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    let column = |title: String, rows: Vec<AnyView>| {
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::TIGHT)
                .width(LengthSpec::Shrink),
        )
        .children((caption(title), rows))
        .into_any()
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::SECTION)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT),
    )
    .children((
        column(t("cheat-markers"), markers),
        column(t("cheat-shortcuts"), keys),
    ))
    .into_any()
}
