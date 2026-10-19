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
/// Global request identities, shared by syncs, listings and consent sessions;
/// replacing an owner never reuses one of its outstanding tickets.
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

mod activation;
pub use activation::Activation;

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
    /// Folders of the package and its dependencies, watched while it runs.
    pub roots: Vec<std::path::PathBuf>,
    /// Exactly the declared data the script may query, from this snapshot.
    pub data: Arc<nanatype_theme::DataAccess>,
}
pub fn review(registry: &SkinRegistry, id: &str) -> Result<ActivationReview, String> {
    let files = registry.package_files(id).map_err(|e| e.to_string())?;
    let project = nanatype_theme::PackageProject::capture(&*files).map_err(|e| e.to_string())?;
    let manifest = project.manifest().map_err(|e| e.to_string())?;
    if !manifest.capabilities.contains(&PackageCapability::Lua) {
        return Err("package does not provide Lua".into());
    }
    let entry = manifest.entry().ok_or("missing Lua entry")?;
    let bytes = project.files.get(entry).ok_or("missing Lua source")?;
    if bytes.len() > nanatype_plugins::MAX_SCRIPT_BYTES {
        return Err("plugin source exceeds budget".into());
    }
    let source = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
    let data = Arc::new(nanatype_theme::DataAccess::for_plugin(&project, &manifest)?);
    Ok(ActivationReview {
        data,
        roots: files.watch_roots(),
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

/// Answer a worker's data requests from one reviewed snapshot, by value.
pub fn data_answer(data: Arc<nanatype_theme::DataAccess>) -> nanatype_plugins::DataAnswer {
    Arc::new(
        move |request| match data.answer(&request.id, &request.query) {
            Ok(value) => nanatype_plugins::DataReply {
                token: request.token,
                value: Some(value),
                error: None,
            },
            Err(error) => nanatype_plugins::DataReply::error(request.token, error),
        },
    )
}

/// Fresh digests for running plugins, computed off the UI thread after their
/// package folders change.
pub fn review_digests(
    registry: &SkinRegistry,
    ids: &[String],
) -> Vec<(String, Result<String, String>)> {
    ids.iter()
        .map(|id| (id.clone(), review(registry, id).map(|r| r.digest)))
        .collect()
}
/// Plugins whose content no longer matches what they were started with.
pub fn changed<'a>(
    running: impl IntoIterator<Item = (&'a str, &'a str)>,
    reviewed: &[(String, Result<String, String>)],
) -> Vec<String> {
    running
        .into_iter()
        .filter(|(id, digest)| {
            reviewed
                .iter()
                .find(|(r, _)| r == id)
                .is_some_and(|(_, current)| current.as_deref() != Ok(*digest))
        })
        .map(|(id, _)| id.to_owned())
        .collect()
}

struct Running {
    generation: u64,
    /// The reviewed content this process was started from.
    digest: String,
    roots: Vec<std::path::PathBuf>,
    process: PluginProcess,
}

/// Reviews finished off the UI thread, tagged with the sync that asked.
pub type Reviews = Vec<(String, Result<Arc<ActivationReview>, String>)>;

/// Run `work` on a short-lived background thread and post its message. Package
/// capture, hashing and signature checks never run on the UI thread.
pub fn background(
    name: &str,
    ctx: &RuntimeProgramContext<Msg>,
    work: impl FnOnce() -> Msg + Send + 'static,
) {
    if let Err(error) = try_background(name, ctx, work) {
        crate::debug_log(&format!("{name} unavailable: {error}"));
    }
}

pub(crate) fn try_background(
    name: &str,
    ctx: &RuntimeProgramContext<Msg>,
    work: impl FnOnce() -> Msg + Send + 'static,
) -> std::io::Result<()> {
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || ctx.dispatch(work()))
        .map(|_| ())
}

/// Package rows with their signature status, for the packages page.
pub fn package_rows(registry: &SkinRegistry) -> Vec<PackageRow> {
    registry
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
        .collect()
}

#[derive(Default)]
pub struct Manager {
    running: HashMap<String, Running>,
    watch: Option<crate::services::watch::Watch>,
    /// The newest sync; reviews from an older one are ignored.
    ticket: u64,
    /// The newest package listing request.
    listing: u64,
}
impl Manager {
    pub fn has_running(&self) -> bool {
        !self.running.is_empty()
    }
    pub fn running(&self) -> Vec<String> {
        self.running.keys().cloned().collect()
    }
    /// Stop plugins whose package or dependencies changed since they started;
    /// they need a fresh review and consent.
    pub fn revalidate(&mut self, reviewed: &[(String, Result<String, String>)], ui: Ui) {
        let stale = changed(
            self.running
                .iter()
                .map(|(id, r)| (id.as_str(), r.digest.as_str())),
            reviewed,
        );
        for id in stale {
            self.remove(&id, Some(ui.now_t("packages-consent-required")), ui);
        }
    }
    /// Watch the folders of every running plugin and its dependencies.
    fn refresh_watch(&mut self, ctx: &RuntimeProgramContext<Msg>) {
        let mut roots: Vec<_> = self
            .running
            .values()
            .flat_map(|r| r.roots.iter().cloned())
            .collect();
        roots.sort();
        roots.dedup();
        if self.watch.as_ref().map(|w| w.paths()) == Some(roots.as_slice())
            || (roots.is_empty() && self.watch.is_none())
        {
            return;
        }
        self.watch = (!roots.is_empty()).then(|| {
            let ctx = ctx.clone();
            crate::services::watch::Watch::start(
                roots,
                move |m| ctx.dispatch(m),
                || Msg::PluginPackagesChanged,
            )
        });
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
    /// List packages and verify their signatures in the background.
    pub fn refresh(&mut self, registry: &SkinRegistry, ctx: &RuntimeProgramContext<Msg>) {
        self.listing = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
        let (registry, ticket) = (registry.clone(), self.listing);
        background("package-list", ctx, move || Msg::PackagesListed {
            ticket,
            rows: package_rows(&registry),
        });
    }
    pub fn listed(&self, ticket: u64, rows: Vec<PackageRow>, ui: Ui) {
        if ticket == self.listing {
            ui.packages.set(Arc::new(rows));
        }
    }
    /// Stop what is no longer approved now; review the rest in the background
    /// and start it from [`Self::start_reviewed`].
    pub fn sync(
        &mut self,
        registry: &SkinRegistry,
        enabled: &[String],
        approvals: &std::collections::BTreeMap<String, nanatype_settings::PluginApproval>,
        ctx: RuntimeProgramContext<Msg>,
        ui: Ui,
    ) {
        self.refresh(registry, &ctx);
        self.running.retain(|id, running| {
            enabled.contains(id)
                && approvals
                    .get(id)
                    .is_some_and(|a| a.matches(&running.digest))
        });
        let mut views = (*ui.plugin_views.get_untracked()).clone();
        views.retain(|id, _| self.running.contains_key(id));
        ui.plugin_views.set(Arc::new(views));
        self.refresh_watch(&ctx);
        self.ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
        let pending: Vec<String> = enabled
            .iter()
            .take(nanatype_settings::MAX_ENABLED_PLUGINS)
            .filter(|id| !self.running.contains_key(*id))
            .cloned()
            .collect();
        if pending.is_empty() {
            return;
        }
        let (registry, ticket) = (registry.clone(), self.ticket);
        background("plugin-review", &ctx, move || Msg::PluginsReviewed {
            ticket,
            reviews: pending
                .into_iter()
                .map(|id| {
                    let reviewed = review(&registry, &id).map(Arc::new);
                    (id, reviewed)
                })
                .collect(),
        });
    }
    /// Start reviewed plugins whose approval still matches their content.
    pub fn start_reviewed(
        &mut self,
        ticket: u64,
        reviews: Reviews,
        enabled: &[String],
        approvals: &std::collections::BTreeMap<String, nanatype_settings::PluginApproval>,
        ctx: RuntimeProgramContext<Msg>,
        ui: Ui,
    ) {
        if ticket != self.ticket {
            return; // A newer sync is reviewing the current state.
        }
        for (id, reviewed) in reviews {
            if !enabled.contains(&id) || self.running.contains_key(&id) {
                continue;
            }
            let result = reviewed.and_then(|reviewed| {
                let executable = std::env::current_exe().map_err(|e| e.to_string())?;
                let plugin_id = id.clone();
                let dispatch = ctx.clone();
                self.start(
                    &reviewed,
                    approvals.get(&id),
                    &executable,
                    move |generation, response| {
                        dispatch.dispatch(Msg::PluginResult {
                            id: plugin_id.clone(),
                            generation,
                            response: Arc::new(response),
                        });
                    },
                    ui,
                )
            });
            if let Err(message) = result {
                self.remove(&id, Some(message), ui);
            }
        }
        self.refresh_watch(&ctx);
    }
    /// Start one immutable snapshot, enforcing approval at the process boundary.
    /// The host supplies its executable and dispatch callback; tests use the
    /// same path with the production worker executable.
    pub fn start(
        &mut self,
        reviewed: &ActivationReview,
        approval: Option<&nanatype_settings::PluginApproval>,
        executable: &std::path::Path,
        post: impl Fn(u64, WorkerResponse) + Send + 'static,
        ui: Ui,
    ) -> Result<u32, String> {
        if !approval_matches(reviewed, approval) {
            return Err(ui.now_t("packages-consent-required"));
        }
        let reads: Vec<_> = reviewed
            .data
            .disclosures()
            .map(|d| d.reference.clone())
            .collect();
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let process = PluginProcess::spawn(
            executable,
            reviewed.source.clone(),
            serde_json::json!({"type": "init", "reads": reads}),
            data_answer(reviewed.data.clone()),
            move |response| post(generation, response),
        )
        .map_err(|e| e.to_string())?;
        let pid = process.id();
        self.running.insert(
            reviewed.id.clone(),
            Running {
                generation,
                digest: reviewed.digest.clone(),
                roots: reviewed.roots.clone(),
                process,
            },
        );
        Ok(pid)
    }
    pub fn event(&mut self, id: &str, event: serde_json::Value) -> Result<(), String> {
        self.running
            .get(id)
            .ok_or_else(|| "plugin is not running".to_owned())?
            .process
            .event(event)
            .map_err(|e| e.to_string())
    }
    /// A plugin that cannot keep up (a busy chat) misses events; only a
    /// stopped worker is removed.
    pub fn broadcast(&mut self, event: serde_json::Value, ui: Ui) {
        let stopped: Vec<_> = self
            .running
            .iter()
            .filter(|(_, running)| {
                running.process.event(event.clone()) == Err(nanatype_plugins::EventError::Stopped)
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
        if !self
            .running
            .get(id)
            .is_some_and(|r| r.generation == generation)
        {
            return;
        }
        let result: Result<Option<&PluginUpdate>, String> = match response {
            WorkerResponse::Update { update, .. } => update.validate().map(|_| Some(update)),
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
