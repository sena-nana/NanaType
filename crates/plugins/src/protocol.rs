use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
pub const MAX_SCRIPT_BYTES: usize = 128 * 1024;
pub const MAX_NODES: usize = 512;
pub const MAX_DRAW_COMMANDS: usize = 2048;
/// Data requests one dispatch may queue.
pub const MAX_DATA_REQUESTS: usize = 8;
/// Chained request/answer rounds per host event; later requests are dropped.
pub const MAX_DATA_ROUNDS: usize = 4;
/// Serialized bytes of one answered value.
pub const MAX_DATA_RESULT_BYTES: usize = 64 * 1024;
/// Serialized bytes of all answers delivered together.
pub const MAX_DATA_EVENT_BYTES: usize = 120 * 1024;
/// Serialized bytes of one request's query.
pub const MAX_DATA_QUERY_BYTES: usize = 1024;

/// A script's request for a declared data resource. Tokens are chosen by the
/// script; answers arrive later as a `data` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataRequest {
    pub token: u32,
    pub id: String,
    pub query: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataReply {
    pub token: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl DataReply {
    pub fn error(token: u32, message: impl Into<String>) -> Self {
        Self {
            token,
            value: None,
            error: Some(message.into()),
        }
    }
}

pub fn validate_requests(requests: &[DataRequest]) -> Result<(), String> {
    if requests.len() > MAX_DATA_REQUESTS {
        return Err(format!(
            "at most {MAX_DATA_REQUESTS} data requests per update"
        ));
    }
    let mut tokens = BTreeSet::new();
    for request in requests {
        if !tokens.insert(request.token) {
            return Err("duplicate data request token".into());
        }
        if request.id.is_empty() || request.id.len() > 128 || !request.query.is_object() {
            return Err("data requests need a short ID and a query table".into());
        }
        if serde_json::to_vec(&request.query).map_or(usize::MAX, |b| b.len()) > MAX_DATA_QUERY_BYTES
        {
            return Err("data query exceeds budget".into());
        }
    }
    Ok(())
}

/// Replace answers over the value or combined budget with errors.
pub fn bound_replies(replies: Vec<DataReply>) -> Vec<DataReply> {
    let mut total = 0;
    replies
        .into_iter()
        .map(|reply| {
            let len = serde_json::to_vec(&reply).map_or(usize::MAX, |b| b.len());
            if len > MAX_DATA_RESULT_BYTES + 256 {
                DataReply::error(reply.token, "data result exceeds budget")
            } else if total + len > MAX_DATA_EVENT_BYTES {
                DataReply::error(reply.token, "data results exceed the per-event budget")
            } else {
                total += len;
                reply
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUpdate {
    #[serde(default)]
    pub views: Vec<PluginView>,
    #[serde(default)]
    pub commands: Vec<PluginCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount {
    Display,
    History,
    Input,
    Panel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginView {
    pub slot: Mount,
    pub root: Node,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommand {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Node {
    Column {
        id: String,
        #[serde(default)]
        children: Vec<Node>,
    },
    Row {
        id: String,
        #[serde(default)]
        children: Vec<Node>,
    },
    Text {
        id: String,
        text: String,
    },
    Button {
        id: String,
        text: String,
        command: String,
    },
    Canvas {
        id: String,
        width: f32,
        height: f32,
        #[serde(default)]
        draw: Vec<DrawCommand>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DrawCommand {
    Rect {
        rect: [f32; 4],
        color: [u8; 4],
        #[serde(default)]
        radius: f32,
    },
    Path {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
        #[serde(default)]
        closed: bool,
        #[serde(default)]
        width: f32,
    },
}

impl PluginUpdate {
    /// Host painting cost of this update: draw commands, with long paths
    /// weighing more. Used to pace repaints of heavy canvases.
    pub fn paint_cost(&self) -> usize {
        fn cost(node: &Node) -> usize {
            match node {
                Node::Column { children, .. } | Node::Row { children, .. } => {
                    children.iter().map(cost).sum()
                }
                Node::Canvas { draw, .. } => draw
                    .iter()
                    .map(|d| match d {
                        DrawCommand::Rect { .. } => 1,
                        DrawCommand::Path { points, .. } => 1 + points.len() / 16,
                    })
                    .sum(),
                Node::Text { .. } | Node::Button { .. } => 0,
            }
        }
        self.views.iter().map(|v| cost(&v.root)).sum()
    }
    pub fn validate(&self) -> Result<(), String> {
        fn id(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        }
        fn finite(value: f32) -> bool {
            value.is_finite() && value.abs() <= 8192.0
        }
        fn node(
            n: &Node,
            depth: usize,
            count: &mut usize,
            draws: &mut usize,
            payload: &mut (usize, usize),
            ids: &mut BTreeSet<String>,
        ) -> Result<(), String> {
            *count += 1;
            let key = match n {
                Node::Column { id, .. }
                | Node::Row { id, .. }
                | Node::Text { id, .. }
                | Node::Button { id, .. }
                | Node::Canvas { id, .. } => id,
            };
            if depth > 32 || *count > MAX_NODES || !id(key) || !ids.insert(key.clone()) {
                return Err("invalid, duplicate or excessive component IDs".into());
            }
            if let Node::Text { text, .. } | Node::Button { text, .. } = n {
                payload.0 += text.len();
            }
            match n {
                Node::Column { children, .. } | Node::Row { children, .. } => {
                    for child in children {
                        node(child, depth + 1, count, draws, payload, ids)?;
                    }
                }
                Node::Text { text, .. } | Node::Button { text, .. } if text.len() > 8192 => {
                    return Err("component text exceeds 8 KiB".into());
                }
                Node::Canvas {
                    width,
                    height,
                    draw,
                    ..
                } => {
                    if !finite(*width) || !finite(*height) || *width <= 0. || *height <= 0. {
                        return Err("invalid canvas dimensions".into());
                    }
                    *draws += draw.len();
                    if *draws > MAX_DRAW_COMMANDS {
                        return Err("too many drawing commands".into());
                    }
                    for d in draw {
                        payload.1 += match d {
                            DrawCommand::Rect { .. } => 4,
                            DrawCommand::Path { points, .. } => points.len(),
                        };
                        match d {
                            DrawCommand::Rect { rect, radius, .. }
                                if !rect.iter().copied().all(finite)
                                    || rect[2] < 0.
                                    || rect[3] < 0.
                                    || !finite(*radius)
                                    || *radius < 0. =>
                            {
                                return Err("invalid rectangle".into());
                            }
                            DrawCommand::Path { points, width, .. }
                                if points.len() > 256
                                    || !points.iter().flatten().copied().all(finite)
                                    || !finite(*width)
                                    || *width < 0. =>
                            {
                                return Err("invalid path".into());
                            }
                            _ => {}
                        }
                    }
                }
                Node::Button { command, .. } if !id(command) => {
                    return Err("invalid command ID".into());
                }
                _ => {}
            }
            if payload.0 > 16 * 1024 || payload.1 > 8192 {
                return Err("plugin text/geometry budget exceeded".into());
            }
            Ok(())
        }
        if self.views.len() > 8 || self.commands.len() > 64 {
            return Err("too many views/commands".into());
        }
        let mut ids = BTreeSet::new();
        let mut count = 0;
        let mut draws = 0;
        let mut payload = (0, 0);
        for view in &self.views {
            node(
                &view.root,
                0,
                &mut count,
                &mut draws,
                &mut payload,
                &mut ids,
            )?;
        }
        let mut commands = BTreeSet::new();
        for command in &self.commands {
            if !id(&command.id) || command.label.len() > 256 || !commands.insert(&command.id) {
                return Err("invalid command".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerRequest {
    Init {
        source: String,
    },
    Event {
        event: serde_json::Value,
    },
    /// Answers to the requests of the previous update.
    Data {
        results: Vec<DataReply>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkerResponse {
    Ready,
    Update {
        update: PluginUpdate,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        requests: Vec<DataRequest>,
    },
    Error {
        message: String,
    },
}

pub fn read_message<T: serde::de::DeserializeOwned>(
    reader: &mut impl std::io::Read,
) -> std::io::Result<T> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_MESSAGE_BYTES {
        return Err(std::io::Error::other("oversized plugin message"));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}
pub fn write_message<T: serde::Serialize>(
    writer: &mut impl std::io::Write,
    value: &T,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(std::io::Error::other("oversized plugin response"));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
