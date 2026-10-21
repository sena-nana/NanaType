//! Round avatars and pictures that update in place as their files arrive.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{Stack, Text};
use nana_ui_core::{
    AlignSpec, BackgroundImage, BackgroundImageFit, JustifySpec, LengthSpec, RadiusTier,
};

use super::Stage;
use super::kit::Type;

/// A round avatar: the name's first letter on the accent colour, with the
/// picture (when there is one) laid over it, so a picture that is still
/// loading, or cannot be read, leaves the initial rather than an empty
/// circle. (NanaUI's `Avatar` samples a host texture and leaves an empty
/// slot a plain grey circle, without initials, so it cannot stand in.)
pub fn asset_avatar(
    ui: Stage,
    asset: Option<String>,
    name: &str,
    size: f32,
    accent: [f32; 4],
) -> AnyView {
    let mut st = Type::Brand.style();
    {
        let l = Arc::make_mut(&mut st.layout);
        // The initial scales with the avatar.
        l.font_size = Some(size * 0.45);
        l.color = Some([1.0; 4]);
    }
    let initial = name.chars().next().map(String::from).unwrap_or_default();
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .radius(RadiusTier::Full)
            .with_layout(move |l| {
                l.position = nana_ui_core::PositionSpec::Relative;
                l.flex_shrink = Some(0.0);
                l.background = Some(accent);
            }),
    )
    .children((
        widget(Text::new(initial).decorative().style(st)),
        reactive_picture(ui, asset, size / 2.0),
    ))
    .into_any()
}

/// Updates only the image node; late downloads never remount a message.
pub fn reactive_picture(ui: Stage, asset: Option<String>, radius: f32) -> AnyView {
    widget(
        super::kit::corner(Stack::row(0.0), radius).with_layout(move |l| {
            l.position = nana_ui_core::PositionSpec::Absolute;
            l.offset_left = Some(LengthSpec::Px(0.0));
            l.offset_top = Some(LengthSpec::Px(0.0));
            l.offset_right = Some(LengthSpec::Px(0.0));
            l.offset_bottom = Some(LengthSpec::Px(0.0));
            l.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
        }),
    )
    .bind(move |s: &mut Stack| {
        let url = asset.as_ref().and_then(|a| {
            ui.asset_urls.with(|u| u.get(a).cloned()).or_else(|| {
                if a.starts_with("blob:") {
                    None
                } else if a.contains("://") || a.starts_with("data:") {
                    Some(a.clone())
                } else {
                    crate::media::url(std::path::Path::new(a), None)
                }
            })
        });
        let image = url.map(|u| BackgroundImage::url_with_fit(u, BackgroundImageFit::Cover));
        s.layout_mut().paint.background_image = image;
    })
    .into_any()
}
