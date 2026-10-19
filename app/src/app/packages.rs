use super::App;
use crate::msg::Msg;
use std::sync::Arc;

impl App {
    /// Plugin consent, package removal and plugin traffic.
    pub(super) fn package_msg(&mut self, msg: Msg) {
        match msg {
            Msg::RequestPluginActivation(id) => match crate::plugins::review(&self.registry, &id) {
                Ok(review) => {
                    self.ui.plugin_source_trusted.set(false);
                    self.ui.plugin_activation.set(Some(Arc::new(review)));
                }
                Err(error) => self.notice(error, true),
            },
            Msg::CancelPluginActivation => {
                self.ui.plugin_source_trusted.set(false);
                self.ui.plugin_activation.set(None);
            }
            Msg::ConfirmPluginActivation => {
                if self.ui.plugin_source_trusted.get_untracked()
                    && let Some(reviewed) = self.ui.plugin_activation.get_untracked()
                {
                    match crate::plugins::review(&self.registry, &reviewed.id) {
                        Ok(current) if current.digest == reviewed.digest => {
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
                                    current.id,
                                    nanatype_settings::PluginApproval {
                                        digest: current.digest,
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
                            self.ui.plugin_activation.set(None);
                            self.ui.plugin_source_trusted.set(false);
                        }
                        Ok(_) => self.notice(self.ui.now_t("packages-changed-during-review"), true),
                        Err(error) => self.notice(error, true),
                    }
                }
            }
            Msg::DisablePlugin(id) => {
                self.plugins.stop(&id, self.ui);
                self.edit(|s| {
                    s.enabled_plugins.retain(|v| v != &id);
                    s.plugin_approvals.remove(&id);
                });
            }
            Msg::RefreshPackages => {
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
                    self.edit(|s| {
                        s.enabled_plugins.retain(|p| p != &id);
                        s.plugin_approvals.remove(&id);
                        if s.skin.id == id {
                            s.skin.id = nanatype_theme::DEFAULT_SKIN.into();
                        }
                    });
                    self.refresh_skin_rows();
                    self.plugins.refresh(&self.registry, self.ui);
                }
                Err(error) => self.notice(error.to_string(), true),
            },
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
}
