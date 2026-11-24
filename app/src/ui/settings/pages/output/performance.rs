use super::super::*;
use crate::ui::problem_text;

/// The GPU to render on, then the scheduling preferences, each notice saying
/// why it is not in effect when the system refused it.
pub(super) fn performance(ui: Ui) -> Vec<Row> {
    let adapter = gpu_adapter(ui);
    let gpu = row_live_notice(
        ui,
        "output-gpu-high",
        move || Some(ui.tr("output-gpu-high-hint", &[])),
        move || {
            let refused = ui.settings.with(|s| s.performance.gpu_high)
                && !ui.performance.with(|p| p.gpu_high);
            refused.then(|| ui.tr("output-not-in-effect", &[]))
        },
        switch(
            ui,
            |s| s.performance.gpu_high,
            |s, v| s.performance.gpu_high = v,
        ),
    );
    let cpu = row_live_notice(
        ui,
        "output-cpu-high",
        move || Some(ui.tr("output-cpu-high-hint", &[])),
        move || {
            let error = ui.performance.with(|p| p.cpu.priority_error.clone())?;
            let error = problem_text(ui, &error);
            Some(ui.tr("output-not-in-effect-because", &[("error", error.into())]))
        },
        switch(
            ui,
            |s| s.performance.cpu_high,
            |s, v| s.performance.cpu_high = v,
        ),
    );
    let affinity = row_live_notice(
        ui,
        "output-cpu-affinity",
        move || Some(ui.tr("output-cpu-affinity-hint", &[])),
        move || {
            let error = ui.performance.with(|p| p.cpu.affinity_error.clone())?;
            Some(problem_text(ui, &error))
        },
        switch(
            ui,
            |s| s.performance.cpu_performance_cores,
            |s, v| s.performance.cpu_performance_cores = v,
        ),
    );
    vec![adapter, gpu, cpu, affinity]
}

/// The GPU choice. The device is created at startup, so the hint names the GPU
/// in use and the row's notice says when a choice waits for a restart, was not
/// found, or is overridden by `WGPU_ADAPTER_NAME`.
fn gpu_adapter(ui: Ui) -> Row {
    let perf = ui.performance.get_untracked();
    let choice = &perf.gpu;
    let mut options = vec![("auto".to_owned(), ui.now_t("output-gpu-adapter-auto"))];
    options.extend(
        choice
            .adapters
            .iter()
            .map(|name| (name.clone(), name.clone())),
    );
    // A GPU chosen before it was removed stays listed, so the select shows it.
    if let Some(saved) = ui.settings.get_untracked().performance.gpu_adapter.clone()
        && !choice.adapters.contains(&saved)
    {
        options.push((saved.clone(), saved));
    }
    row_live_notice(
        ui,
        "output-gpu-adapter",
        move || {
            let saved = ui.settings.with(|s| s.performance.gpu_adapter.clone());
            let (choice, running) = ui
                .performance
                .with(|p| (p.gpu.clone(), p.gpu_running.clone()));
            let missing = saved
                .as_ref()
                .is_some_and(|saved| !choice.adapters.contains(saved));
            if choice.external || missing {
                return None;
            }
            let name = running.unwrap_or_else(|| "…".into());
            Some(ui.tr("output-gpu-adapter-hint", &[("name", name.into())]))
        },
        move || {
            let saved = ui.settings.with(|s| s.performance.gpu_adapter.clone());
            let (choice, running) = ui
                .performance
                .with(|p| (p.gpu.clone(), p.gpu_running.clone()));
            let name = running.unwrap_or_else(|| "…".into());
            let args = [("name", name.into())];
            if choice.external {
                Some(ui.tr("output-gpu-adapter-env", &args))
            } else if saved
                .as_ref()
                .is_some_and(|saved| !choice.adapters.contains(saved))
            {
                Some(ui.tr("output-gpu-adapter-missing", &args))
            } else if saved != choice.applied {
                Some(ui.tr("output-gpu-adapter-restart", &[]))
            } else {
                None
            }
        },
        select(
            ui,
            options,
            |s| {
                s.performance
                    .gpu_adapter
                    .clone()
                    .unwrap_or_else(|| "auto".into())
            },
            |s, v| s.performance.gpu_adapter = (v != "auto").then(|| v.to_owned()),
        ),
    )
}
