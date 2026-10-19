use super::App;
use crate::msg::Msg;
use std::sync::Arc;

impl App {
    /// Plugin consent, package removal and plugin traffic.
    pub(super) fn package_msg(&mut self, msg: Msg) {
        match msg {
            Msg::RequestPluginActivation(id) => {
                let ticket = self.plugin_activation.request(id.clone(), self.ui);
                self.review_plugin(ticket, id);
            }
            Msg::CancelPluginActivation => self.plugin_activation.cancel(self.ui),
            Msg::ConfirmPluginActivation => {
                if let Some((ticket, reviewed)) = self.plugin_activation.confirm(self.ui) {
                    self.review_plugin(ticket, reviewed.id.clone());
                }
            }
            Msg::PluginReviewReady { ticket, result } => {
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
            Msg::PluginsReviewed { ticket, reviews } => self.plugins.start_reviewed(
                ticket,
                reviews,
                &self.settings.enabled_plugins,
                &self.settings.plugin_approvals,
                self.ctx.clone(),
                self.ui,
            ),
            Msg::PackagesListed { ticket, rows } => self.plugins.listed(ticket, rows, self.ui),
            Msg::PackageInstalled(result) => self.package_installed(result),
            Msg::PackageExported(result) => {
                if let Err(e) = result {
                    let text = self.ui.tr("skins-export-failed", &[("message", e.into())]);
                    self.notice(text, true);
                }
            }
            Msg::DisablePlugin(id) => {
                self.plugin_activation.cancel_for(&id, self.ui);
                self.plugins.stop(&id, self.ui);
                self.edit(|s| {
                    s.enabled_plugins.retain(|v| v != &id);
                    s.plugin_approvals.remove(&id);
                });
            }
            Msg::RefreshPackages => {
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
            Msg::RemovePackage(id) => match self.registry.remove(&id) {
                Ok(()) => {
                    self.plugin_activation.cancel(self.ui);
                    self.edit(|s| {
                        s.enabled_plugins.retain(|p| p != &id);
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
            Msg::PluginPackagesChanged => {
                let registry = self.registry.clone();
                let ids = self.plugins.running();
                crate::plugins::background("plugin-review", &self.ctx, move || {
                    Msg::PluginPackagesReviewed(crate::plugins::review_digests(&registry, &ids))
                });
            }
            Msg::PluginPackagesReviewed(reviewed) => self.plugins.revalidate(&reviewed, self.ui),
            Msg::PluginResult {
                id,
                generation,
                response,
            } => self.plugins.receive(&id, generation, &response, self.ui),
            Msg::PluginCommand { id, command } => {
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
            Msg::PluginInput { id, event } => {
                let _ = self.plugins.event(&id, event);
            }
            _ => unreachable!("not a package message"),
        }
    }

    fn review_plugin(&mut self, ticket: u64, id: String) {
        let registry = self.registry.clone();
        if let Err(error) = crate::plugins::try_background("plugin-review", &self.ctx, move || {
            Msg::PluginReviewReady {
                ticket,
                result: crate::plugins::review(&registry, &id).map(Arc::new),
            }
        }) {
            self.package_msg(Msg::PluginReviewReady {
                ticket,
                result: Err(error.to_string()),
            });
        }
    }
}
