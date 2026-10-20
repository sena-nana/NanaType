//! The application's typed tokens: one name per decision, so two views that
//! mean the same thing cannot drift to different numbers. Every value sits
//! on NanaUI's scales ([`space`], [`type_scale`]); views name the role, never
//! the number.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, Text};
use nana_ui::theme::{space, type_scale};
use nana_ui_core::{LengthSpec, SemanticColorRole};

/// A text role: size, weight and (for the quiet roles) colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// A window's brand line ("NanaType" beside the logo): 16/700.
    Brand,
    /// A page's title: 18/600.
    PageTitle,
    /// A panel's or a dialog's title, a hero line: 16/600.
    Title,
    /// A strong item name inside a card: 13/600.
    Strong,
    /// A row's label: 13/500.
    Label,
    /// Running text: 13/400.
    Body,
    /// Running text in the muted colour: 13/400.
    Lead,
    /// A section's title above its card, a column caption: 12/600, muted.
    Overline,
    /// A small label (a tag, a count, a slot): 12/500.
    Small,
    /// A hint under a label, a status line: 12/400, muted.
    Caption,
    /// The smallest text (a badge, a footnote): 11/500.
    Micro,
    /// Chat text: 14/400.
    Chat,
}

impl Type {
    pub const fn size(self) -> f32 {
        match self {
            Self::Brand | Self::Title => type_scale::HEADING,
            Self::PageTitle => type_scale::TITLE,
            Self::Chat => type_scale::SECTION,
            Self::Strong | Self::Label | Self::Body | Self::Lead => type_scale::BODY,
            Self::Overline | Self::Small | Self::Caption => type_scale::META,
            Self::Micro => type_scale::HINT,
        }
    }

    pub const fn weight(self) -> u16 {
        match self {
            Self::Brand => type_scale::BOLD,
            Self::PageTitle | Self::Title | Self::Strong | Self::Overline => type_scale::SEMIBOLD,
            Self::Label | Self::Small | Self::Micro => type_scale::MEDIUM,
            Self::Body | Self::Lead | Self::Caption | Self::Chat => type_scale::REGULAR,
        }
    }

    /// The colour the role is drawn in, when it is not the text colour.
    pub const fn role(self) -> Option<SemanticColorRole> {
        match self {
            Self::Lead | Self::Overline | Self::Caption => Some(SemanticColorRole::Muted),
            _ => None,
        }
    }

    /// The role's size and weight (not its colour).
    pub fn style(self) -> NodeStyle {
        let mut st = NodeStyle::default();
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(self.size());
        l.font_weight = Some(self.weight());
        st
    }

    /// [`Self::style`] in the mono face with tabular figures, for text typed
    /// literally (ports, addresses, versions, IDs).
    pub fn mono(self) -> NodeStyle {
        let mut st = self.style();
        crate::ui::mono(Arc::make_mut(&mut st.layout));
        st
    }

    /// [`Self::style`] on one line, cut with an ellipsis where it does not
    /// fit, taking what room the row leaves it.
    pub fn one_line(self) -> NodeStyle {
        let mut st = self.style();
        let l = Arc::make_mut(&mut st.layout);
        l.white_space_nowrap = true;
        l.text_overflow_ellipsis = true;
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        st
    }

    /// A text node in this role, in its colour.
    pub fn text(self, value: impl Into<String>) -> Text {
        let t = Text::new(value.into()).style(self.style());
        match self.role() {
            Some(role) => t.color(role),
            None => t,
        }
    }

    /// [`Self::text`] as a view.
    pub fn view(self, value: impl Into<String>) -> AnyView {
        widget(self.text(value)).into_any()
    }
}

/// Gaps between siblings. New layouts use these six steps only.
pub mod gap {
    use super::space;
    /// Between the marks of one control (an icon and its count).
    pub const HAIR: f32 = space::XXS;
    /// Between a label and its hint, or tightly coupled items.
    pub const TIGHT: f32 = space::XS;
    /// Between controls in a row.
    pub const CONTROL: f32 = space::MD;
    /// Between groups inside a card or a bar.
    pub const GROUP: f32 = space::XL;
    /// Between cards and sections.
    pub const SECTION: f32 = space::XXXL;
    /// Between a page's major regions.
    pub const PAGE: f32 = space::PAGE;
}

/// Insets (padding) of the app's containers.
pub mod inset {
    use super::space;
    /// The horizontal inset of everything inside a card.
    pub const CARD: f32 = space::XXXL;
    /// The vertical padding of a block or plain row inside a card.
    pub const ROW_Y: f32 = space::XL;
    /// A bar's (toolbar, footer, header) horizontal padding.
    pub const BAR_X: f32 = space::XL;
    /// A bar's vertical padding.
    pub const BAR_Y: f32 = space::MD;
    /// A popover's or a compact panel's padding.
    pub const PANEL: f32 = space::XL;
    /// A settings page's padding.
    pub const PAGE: f32 = space::PAGE;
    /// A chip's horizontal padding.
    pub const CHIP_X: f32 = space::MD;
    /// A chip's vertical padding.
    pub const CHIP_Y: f32 = space::XXS;
}

/// Widths of fields and other fixed-width controls.
pub mod field {
    /// A port, a short number.
    pub const XS: f32 = 96.0;
    /// A number, a short name.
    pub const SM: f32 = 160.0;
    /// A select, a slider, a name.
    pub const MD: f32 = 240.0;
}

/// Icon sizes.
pub mod icon {
    /// Inline with small text, inside keycaps.
    pub const SM: f32 = 12.0;
    /// Beside body text, in toolbars.
    pub const MD: f32 = 16.0;
    /// A section's or an empty state's lead glyph.
    pub const LG: f32 = 20.0;
}

/// The settings window's sidebar width.
pub const SIDEBAR_W: f32 = 216.0;
/// The widest a settings page's content grows.
pub const PAGE_MAX_W: f32 = 720.0;
/// The height of a settings page's fixed header.
pub const PAGE_HEADER_H: f32 = 64.0;
/// A small status dot.
pub const DOT: f32 = 8.0;
/// The height of a pill (a status badge with a dot, a filter chip).
pub const PILL_H: f32 = 22.0;

/// What a mark says about its subject: drawn as a colour role everywhere
/// (dots, pills, tags, callouts), so "connected" is the same green in the
/// chat header, a settings badge and the package list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
    /// Off, idle: the faint grey.
    Off,
}

impl Tone {
    /// The tone's mark colour (a dot, an icon, a pill's text).
    pub const fn role(self) -> SemanticColorRole {
        match self {
            Self::Neutral => SemanticColorRole::Muted,
            Self::Accent => SemanticColorRole::Accent,
            Self::Success => SemanticColorRole::Success,
            Self::Warning => SemanticColorRole::Warning,
            Self::Danger => SemanticColorRole::Danger,
            Self::Off => SemanticColorRole::Faint,
        }
    }

    /// The tone's text on its own 12% wash (a pill's label): the accent
    /// darkened where it would not read as text.
    pub const fn ink(self) -> SemanticColorRole {
        match self {
            Self::Accent => SemanticColorRole::AccentOnSoft,
            Self::Off => SemanticColorRole::Muted,
            other => other.role(),
        }
    }
}
