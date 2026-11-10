use super::App;
use crate::msg::{Msg, PackageMsg};
use std::sync::Arc;

impl App {
    /// Switch package `id` on or off. Off takes its skin and sound pack out
    /// of the pickers (a choice that named one falls back) and stops its
    /// script, whose consent goes with it. On brings them back; a script
    /// still has to be reviewed before it runs. The built-in package stays on.
    fn set_package_enabled(&mut self, id: String, on: bool) {
        let Some((manifest, origin)) = self
            .registry
            .packages()
            .into_iter()
            .find(|(m, _)| m.meta.id == id)
        else {
            return;
        };
        if origin == nanatype_theme::SkinOrigin::Builtin {
            return;
        }
        let functional = manifest
            .capabilities
            .contains(&nanatype_theme::PackageCapability::Lua);
        if on {
            self.edit(|s| s.disabled_packages.retain(|p| p != &id));
            self.refresh_skin_rows();
            self.sync_tray_skins();
            if functional {
                self.package_msg(PackageMsg::RequestActivation(id));
            }
            return;
        }
        self.plugin_activation.cancel_for(&id, self.ui);
        self.plugins.stop(&id, self.ui);
        self.edit(|s| {
            s.enabled_plugins.retain(|p| p != &id);
            s.plugin_approvals.remove(&id);
            if !s.disabled_packages.contains(&id) {
                s.disabled_packages.push(id.clone());
            }
            if s.skin.id == id {
                s.skin.id = nanatype_theme::DEFAULT_SKIN.into();
            }
            if s.sound.pack.as_ref() == Some(&id) {
                s.sound.pack = None;
            }
        });
        self.refresh_skin_rows();
        self.sync_tray_skins();
    }

    /// Plugin consent, package removal and plugin traffic.
    pub(super) fn package_msg(&mut self, msg: PackageMsg) {
        match msg {
            PackageMsg::RequestActivation(id) => {
                let ticket = self.plugin_activation.request(id.clone(), self.ui);
                self.review_plugin(ticket, id);
            }
            PackageMsg::CancelActivation => self.plugin_activation.cancel(self.ui),
            PackageMsg::ConfirmActivation => {
                if let Some((ticket, reviewed)) = self.plugin_activation.confirm(self.ui) {
                    self.review_plugin(ticket, reviewed.id.clone());
                }
            }
            PackageMsg::ReviewReady { ticket, result } => {
                match self.plugin_activation.complete(ticket, result, self.ui) {
                    Ok(Some(current)) => {
                        let unchanged = self.settings.enabled_plugins.contains(&current.id)
                            && crate::plugins::approval_matches(
                                &current,
                                self.settings.plugin_approvals.get(&current.id),
                            );
                        self.plugins.stop(&current.id, self.ui);
                        self.edit(|s| {
                            if !s.enabled_plugins.contains(&current.id) {
                                s.enabled_plugins.push(current.id.clone());
                            }
                            s.plugin_approvals.insert(
                                current.id.clone(),
                                nanatype_settings::PluginApproval {
                                    digest: current.digest.clone(),
                                    trusted_source: true,
                                    policy: nanatype_settings::PLUGIN_CONSENT_POLICY,
                                },
                            );
                        });
                        // Reconfirming identical bytes must also restart a stopped or failed worker.
                        if unchanged {
                            self.plugins.sync(
                                &self.registry,
                                &self.settings.enabled_plugins,
                                &self.settings.plugin_approvals,
                                self.ctx.clone(),
                                self.ui,
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => self.notice(error, true),
                }
            }
            PackageMsg::PluginsReviewed { ticket, reviews } => self.plugins.start_reviewed(
                ticket,
                reviews,
                &self.settings.enabled_plugins,
                &self.settings.plugin_approvals,
                self.ctx.clone(),
                self.ui,
            ),
            PackageMsg::Listed { ticket, rows } => self.plugins.listed(ticket, rows, self.ui),
            PackageMsg::Installed(result) => self.package_installed(result),
            PackageMsg::Exported(result) => {
                if let Err(e) = result {
                    let text = self.ui.tr("skins-export-failed", &[("message", e.into())]);
                    self.notice(text, true);
                }
            }
            PackageMsg::DisablePlugin(id) => {
                self.plugin_activation.cancel_for(&id, self.ui);
                self.plugins.stop(&id, self.ui);
                self.edit(|s| {
                    s.enabled_plugins.retain(|v| v != &id);
                    s.plugin_approvals.remove(&id);
                });
            }
            PackageMsg::SetEnabled(id, on) => self.set_package_enabled(id, on),
            PackageMsg::Refresh => {
                self.plugin_activation.cancel(self.ui);
                self.registry.rescan();
                self.refresh_skin_rows();
                self.plugins = Default::default();
                self.plugins.sync(
                    &self.registry,
                    &self.settings.enabled_plugins,
                    &self.settings.plugin_approvals,
                    self.ctx.clone(),
                    self.ui,
                );
            }
            PackageMsg::Remove(id) => match self.registry.remove(&id) {
                Ok(()) => {
                    self.plugin_activation.cancel(self.ui);
                    self.edit(|s| {
                        s.enabled_plugins.retain(|p| p != &id);
                        s.disabled_packages.retain(|p| p != &id);
                        s.plugin_approvals.remove(&id);
                        if s.skin.id == id {
                            s.skin.id = nanatype_theme::DEFAULT_SKIN.into();
                        }
                    });
                    self.refresh_skin_rows();
                    self.plugins.refresh(&self.registry, &self.ctx);
                }
                Err(error) => self.notice(error.to_string(), true),
            },
            PackageMsg::PluginPackagesChanged => {
                let registry = self.registry.clone();
                let ids = self.plugins.running();
                crate::plugins::background("plugin-review", &self.ctx, move || {
                    Msg::Package(PackageMsg::PluginPackagesReviewed(
                        crate::plugins::review_digests(&registry, &ids),
                    ))
                });
            }
            PackageMsg::PluginPackagesReviewed(reviewed) => {
                self.plugins.revalidate(&reviewed, self.ui)
            }
            PackageMsg::PluginResult {
                id,
                generation,
                response,
            } => self.plugins.receive(&id, generation, &response, self.ui),
            PackageMsg::PluginCommand { id, command } => {
                let allowed = self.ui.plugin_views.with_untracked(|views| {
                    views
                        .get(&id)
                        .is_some_and(|u| u.commands.iter().any(|c| c.id == command))
                });
                if allowed {
                    let _ = self
                        .plugins
                        .event(&id, serde_json::json!({"type":"command","id":command}));
                }
            }
            PackageMsg::PluginInput { id, event } => {
                let _ = self.plugins.event(&id, event);
            }
        }
    }

    fn review_plugin(&mut self, ticket: u64, id: String) {
        let registry = self.registry.clone();
        if let Err(error) = crate::plugins::try_background("plugin-review", &self.ctx, move || {
            Msg::Package(PackageMsg::ReviewReady {
                ticket,
                result: crate::plugins::review(&registry, &id).map(Arc::new),
            })
        }) {
            self.package_msg(PackageMsg::ReviewReady {
                ticket,
                result: Err(error.to_string()),
            });
        }
    }
}
