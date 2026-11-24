//! Isolated Lua debugger running in a sandboxed worker process.
use crate::validation::lua_error_location;
use nanatype_plugins::{PluginUpdate, WorkerResponse};
use nanatype_theme::PackageProject;
use nanatype_theme::editor::SourceDiagnostic;
use std::{collections::VecDeque, path::Path, sync::Arc};

const LOG_LINES: usize = 40;

/// What the debug panel shows. Its views never reach the real plugin slots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DebugView {
    pub trusted: bool,
    pub running: bool,
    pub replaying: bool,
    pub update: Option<PluginUpdate>,
    pub error: Option<String>,
    /// Where in the entry file the error happened, when Lua says so.
    pub location: Option<SourceDiagnostic>,
    /// Recent events sent and results received, newest last.
    pub log: Vec<String>,
}

/// A script under test, in its own sandboxed worker process.
pub struct Debugger {
    /// The project digest the user trusted; any edit revokes it.
    trusted: Option<String>,
    generation: u64,
    process: Option<nanatype_frame::plugin_host::Worker>,
    view: DebugView,
    events: VecDeque<serde_json::Value>,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

impl Debugger {
    pub fn new() -> Self {
        Self {
            trusted: None,
            generation: 0,
            process: None,
            view: DebugView::default(),
            events: VecDeque::new(),
        }
    }

    pub fn view(&self) -> DebugView {
        DebugView {
            trusted: self.trusted.is_some(),
            ..self.view.clone()
        }
    }

    pub fn location(&self) -> Option<&SourceDiagnostic> {
        self.view.location.as_ref()
    }

    pub fn running(&self) -> bool {
        self.process.is_some()
    }

    pub fn on_project_changed(&mut self) {
        self.view.location = None;
        self.view.error = None;
        // No rehash per keystroke: any change revokes the trust.
        if self.trusted.is_some() {
            self.trusted = None;
            if self.process.is_some() {
                self.stop_worker();
                self.log("■ stopped: the package changed".into());
            }
        }
    }

    /// Trust is bound to this exact content and needs a fresh confirmation
    /// after any edit.
    pub fn trust(&mut self, project: &PackageProject, trusted: bool) {
        self.trusted = trusted.then(|| project.digest());
        if !trusted {
            self.stop_worker();
        }
    }

    /// Start the script in its own sandboxed worker. Data answers come from
    /// this snapshot only; results stay in the editor.
    pub fn start(
        &mut self,
        project: &PackageProject,
        executable: &Path,
        generation: u64,
        post: impl Fn(u64, WorkerResponse) + Send + Sync + 'static,
    ) -> Result<(), String> {
        let digest = project.digest();
        if self.trusted.as_deref() != Some(digest.as_str()) {
            return Err("confirm that you trust this script first".into());
        }
        let manifest = project.manifest().map_err(|e| e.to_string())?;
        let entry = manifest.entry().ok_or("the package has no Lua entry")?;
        let bytes = project.files.get(entry).ok_or("missing Lua source")?;
        if bytes.len() > nanatype_plugins::MAX_SCRIPT_BYTES {
            return Err("plugin source exceeds budget".into());
        }
        let source = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
        let data = Arc::new(
            nanatype_theme::DataAccess::for_plugin(project, &manifest)
                .map_err(|e| e.to_string())?,
        );
        let reads: Vec<_> = data.disclosures().map(|d| d.reference.clone()).collect();
        let answer = nanatype_frame::plugin_host::data_answer(data);
        self.stop_worker();
        self.process = Some(nanatype_frame::plugin_host::Worker::spawn(
            executable,
            source,
            serde_json::json!({"type": "init", "reads": reads}),
            answer,
            move |response| post(generation, response),
        )?);
        self.generation = generation;
        self.view = DebugView {
            running: true,
            log: std::mem::take(&mut self.view.log),
            ..Default::default()
        };
        self.log("▶ init".into());
        Ok(())
    }

    pub fn stop(&mut self) {
        if self.process.is_some() {
            self.stop_worker();
            self.log("■ stopped".into());
        }
    }

    pub fn event(&mut self, event: serde_json::Value) -> Result<(), String> {
        if self.view.replaying {
            return Err("wait for the current replay to finish".into());
        }
        self.send_event(event)
    }

    pub fn sequence(&mut self, events: Vec<serde_json::Value>) -> Result<(), String> {
        if self.view.update.is_none() || !self.running() || self.view.replaying {
            return Err("wait for the debugger to be ready".into());
        }
        if events.is_empty()
            || events.len() > 32
            || events
                .iter()
                .any(|e| !e.get("type").is_some_and(|v| v.is_string()))
        {
            return Err("expected 1–32 event objects with a string type".into());
        }
        self.events = events.into();
        self.view.replaying = true;
        self.replay_next()
    }

    pub fn clear(&mut self) {
        self.view.error = None;
        self.view.location = None;
        self.view.log.clear();
    }

    /// A result from the debug worker; stale generations are ignored.
    pub fn result(&mut self, project: &PackageProject, generation: u64, response: &WorkerResponse) {
        if generation == 0 || generation != self.generation {
            return;
        }
        match response {
            WorkerResponse::Ready => {}
            WorkerResponse::Update { update, .. } => match update.validate() {
                Ok(()) => {
                    self.log(format!(
                        "← {} view(s), {} command(s)",
                        update.views.len(),
                        update.commands.len()
                    ));
                    self.view.update = Some(update.clone());
                    if self.view.replaying
                        && let Err(e) = self.replay_next()
                    {
                        self.failed(project, e);
                    }
                }
                Err(error) => self.failed(project, error),
            },
            WorkerResponse::Error { message } => self.failed(project, message.clone()),
            // What the script wrote with `nana.log`, every level: this is the
            // author's own console.
            WorkerResponse::Log { lines, dropped } => {
                for line in lines {
                    let level = ["debug", "info", "warn", "error"]
                        .get(usize::from(line.level))
                        .unwrap_or(&"error");
                    self.log(format!("▸ {level}: {}", line.message));
                }
                if *dropped > 0 {
                    self.log(format!("▸ {dropped} more line(s) over the budget"));
                }
            }
        }
    }

    fn stop_worker(&mut self) {
        self.process = None;
        self.generation = 0;
        self.view.running = false;
        self.view.replaying = false;
        self.events.clear();
    }

    fn log(&mut self, line: String) {
        self.view.log.push(line.chars().take(400).collect());
        let excess = self.view.log.len().saturating_sub(LOG_LINES);
        self.view.log.drain(..excess);
    }

    fn replay_next(&mut self) -> Result<(), String> {
        if let Some(event) = self.events.pop_front() {
            self.send_event(event)
        } else {
            self.view.replaying = false;
            Ok(())
        }
    }

    fn send_event(&mut self, event: serde_json::Value) -> Result<(), String> {
        let process = self.process.as_ref().ok_or("the script is not running")?;
        let line = format!("→ {event}");
        process.event(event).map_err(|e| e.to_string())?;
        self.log(line);
        Ok(())
    }

    fn failed(&mut self, project: &PackageProject, message: String) {
        self.log(format!("✕ {message}"));
        self.view.location = project
            .manifest()
            .ok()
            .and_then(|m| m.lua.map(|l| l.entry))
            .and_then(|entry| {
                let text = std::str::from_utf8(project.files.get(&entry)?).ok()?;
                lua_error_location(&entry, text, &message, true)
            });
        self.view.error = Some(message);
        self.stop_worker();
    }
}

/// Sample events the debug panel can send, as the host would produce them.
pub fn sample_events(text: &str) -> Vec<(&'static str, serde_json::Value)> {
    let markup = nanatype_markup::serialize(&nanatype_markup::parse(text).doc);
    let bridge = |e: nanatype_model::BridgeEvent| serde_json::to_value(e).unwrap_or_default();
    vec![
        (
            "line_start",
            bridge(nanatype_model::BridgeEvent::LineStart {
                id: 1,
                text: text.into(),
                markup,
                speaker: "Nana".into(),
            }),
        ),
        (
            "line_end",
            bridge(nanatype_model::BridgeEvent::LineEnd { id: 1 }),
        ),
        (
            "speaking",
            bridge(nanatype_model::BridgeEvent::Speaking { speaking: true }),
        ),
        (
            "chat_message",
            serde_json::json!({"type": "chat_message", "platform": "blivechat", "author": "viewer", "text": text}),
        ),
    ]
}
