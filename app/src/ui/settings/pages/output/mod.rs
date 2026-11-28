mod capture;
mod obs_wizard;
mod performance;
mod spout;

use super::*;

pub(super) use self::capture::display_source;
pub(super) use self::obs_wizard::{
    obs_address, obs_password_field, sample_step, shared_steps, step, step_text,
};

/// How an output's settings are read and written.
type Read = fn(&Settings) -> &nanatype_settings::SpoutOutput;
type Write = fn(&mut Settings) -> &mut nanatype_settings::SpoutOutput;

pub(super) fn output(ui: Ui) -> AnyView {
    let senders = [
        spout::spout_rows(ui, WindowKind::Display, "output-display", None),
        spout::spout_rows(ui, WindowKind::History, "output-history", None),
        spout::spout_rows(
            ui,
            WindowKind::Danmaku,
            "output-danmaku",
            Some(|s| s.history.chat.separate),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(ui, "output-obs-title", vec![obs_wizard::obs_guide(ui)]),
            obs_wizard::obs_link(ui),
            section_with(
                ui,
                if cfg!(target_os = "macos") {
                    "output-syphon"
                } else {
                    "output-spout"
                },
                senders,
            ),
            // GPU and CPU scheduling are Windows controls.
            cfg!(windows).then(|| section(ui, "output-performance", performance::performance(ui))),
        ))
        .into_any()
}
