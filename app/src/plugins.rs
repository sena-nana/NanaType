//! Package catalogue and per-plugin process lifetimes. Only local UI events return.
use crate::{msg::Msg, ui::Ui};
use nana_ui::RuntimeProgramContext;
use nanatype_plugins::{PluginProcess, PluginUpdate, WorkerResponse};
use nanatype_theme::{PackageCapability, PackageManifest, SkinOrigin, SkinRegistry};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq)]
pub struct PackageRow {
    pub manifest: PackageManifest,
    pub origin: SkinOrigin,
    pub integrity: Result<Option<nanatype_theme::SealStatus>, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivationReview {
    pub id: String,
    pub manifest: PackageManifest,
    pub digest: String,
    pub integrity: Option<nanatype_theme::SealStatus>,
    pub source: String,
    pub location: String,
}
pub fn review(registry: &SkinRegistry, id: &str) -> Result<ActivationReview, String> {
    let files = registry.package_files(id).map_err(|e| e.to_string())?;
    let project = nanatype_theme::PackageProject::capture(&*files).map_err(|e| e.to_string())?;
    let manifest = project.manifest().map_err(|e| e.to_string())?;
    if !manifest.capabilities.contains(&PackageCapability::Lua) {
        return Err("package does not provide Lua".into());
    }
    let entry = manifest.entry.as_deref().ok_or("missing Lua entry")?;
    let bytes = project.files.get(entry).ok_or("missing Lua source")?;
    if bytes.len() > nanatype_plugins::MAX_SCRIPT_BYTES {
        return Err("plugin source exceeds budget".into());
    }
    let source = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
    Ok(ActivationReview {
        id: id.into(),
        digest: project.digest(),
        integrity: nanatype_theme::verify_integrity(&project).map_err(|e| e.to_string())?,
        manifest,
        source,
        location: files
            .root()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "builtin".into()),
    })
}
pub fn approval_matches(
    review: &ActivationReview,
    approval: Option<&nanatype_settings::PluginApproval>,
) -> bool {
    approval.is_some_and(|a| a.matches(&review.digest))
}

#[derive(Default)]
pub struct Manager {
    running: HashMap<String, (u64, PluginProcess)>,
}
impl Manager {
    pub fn has_running(&self) -> bool {
        !self.running.is_empty()
    }
    pub fn stop(&mut self, id: &str, ui: Ui) {
        self.remove(id, None, ui);
    }
    fn remove(&mut self, id: &str, error: Option<String>, ui: Ui) {
        self.running.remove(id);
        ui.plugin_views.update(|views| {
            Arc::make_mut(views).remove(id);
        });
        ui.plugin_errors.update(|errors| {
            let errors = Arc::make_mut(errors);
            match error {
                Some(message) => {
                    errors.insert(id.into(), message);
                }
                None => {
                    errors.remove(id);
                }
            }
        });
    }
    pub fn refresh(&self, registry: &SkinRegistry, ui: Ui) {
        let rows = registry
            .packages()
            .into_iter()
            .map(|(manifest, origin)| {
                let integrity = registry
                    .package_files(&manifest.meta.id)
                    .and_then(|files| nanatype_theme::verify_integrity(&*files))
                    .map_err(|e| e.to_string());
                PackageRow {
                    manifest,
                    origin,
                    integrity,
                }
            })
            .collect();
        ui.packages.set(Arc::new(rows));
    }
    pub fn sync(
        &mut self,
        registry: &SkinRegistry,
        enabled: &[String],
        approvals: &std::collections::BTreeMap<String, nanatype_settings::PluginApproval>,
        ctx: RuntimeProgramContext<Msg>,
        ui: Ui,
    ) {
        self.refresh(registry, ui);
        self.running.retain(|id, _| {
            enabled.contains(id)
                && approvals
                    .get(id)
                    .is_some_and(nanatype_settings::PluginApproval::is_current)
        });
        let mut views = (*ui.plugin_views.get_untracked()).clone();
        views.retain(|id, _| self.running.contains_key(id));
        ui.plugin_views.set(Arc::new(views));
        for id in enabled.iter().take(nanatype_settings::MAX_ENABLED_PLUGINS) {
            if self.running.contains_key(id) {
                continue;
            }
            let prepare = || -> Result<String, String> {
                let reviewed = review(registry, id)?;
                if !approval_matches(&reviewed, approvals.get(id)) {
                    return Err(ui.now_t("packages-consent-required"));
                }
                Ok(reviewed.source)
            };
            let result = prepare().and_then(|source| {
                let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
                let plugin_id = id.clone();
                let dispatch = ctx.clone();
                PluginProcess::spawn(
                    &std::env::current_exe().map_err(|e| e.to_string())?,
                    source,
                    move |response| {
                        dispatch.dispatch(Msg::PluginResult {
                            id: plugin_id.clone(),
                            generation,
                            response: Arc::new(response),
                        });
                    },
                )
                .map(|process| (generation, process))
                .map_err(|e| e.to_string())
            });
            match result {
                Ok(process) => {
                    self.running.insert(id.clone(), process);
                }
                Err(message) => {
                    self.remove(id, Some(message), ui);
                }
            }
        }
    }
    pub fn event(&mut self, id: &str, event: serde_json::Value) -> Result<(), String> {
        self.running
            .get(id)
            .ok_or_else(|| "plugin is not running".to_owned())?
            .1
            .event(event)
            .map_err(|e| e.to_string())
    }
    /// A plugin that cannot keep up (a busy chat) misses events; only a
    /// stopped worker is removed.
    pub fn broadcast(&mut self, event: serde_json::Value, ui: Ui) {
        let stopped: Vec<_> = self
            .running
            .iter()
            .filter(|(_, (_, process))| {
                process.event(event.clone()) == Err(nanatype_plugins::EventError::Stopped)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in stopped {
            self.remove(
                &id,
                Some(nanatype_plugins::EventError::Stopped.to_string()),
                ui,
            );
        }
    }

    pub fn receive(&mut self, id: &str, generation: u64, response: &WorkerResponse, ui: Ui) {
        if !self.running.get(id).is_some_and(|(g, _)| *g == generation) {
            return;
        }
        let result: Result<Option<&PluginUpdate>, String> = match response {
            WorkerResponse::Update { update } => update.validate().map(|_| Some(update)),
            WorkerResponse::Error { message } => Err(message.clone()),
            WorkerResponse::Ready => Ok(None),
        };
        match result {
            Ok(Some(update)) => {
                if !ui
                    .plugin_views
                    .with_untracked(|views| views.get(id) == Some(update))
                {
                    ui.plugin_views.update(|views| {
                        Arc::make_mut(views).insert(id.into(), update.clone());
                    });
                }
                if ui
                    .plugin_errors
                    .with_untracked(|errors| errors.contains_key(id))
                {
                    ui.plugin_errors.update(|errors| {
                        Arc::make_mut(errors).remove(id);
                    });
                }
            }
            Ok(None) => {}
            Err(message) => {
                self.remove(id, Some(message), ui);
            }
        }
    }
}
