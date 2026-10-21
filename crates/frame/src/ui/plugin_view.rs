//! Host-owned widgets and paths generated from a validated, retained value
//! tree: plugins' views on the display and history, and the package editor's
//! debugger.
use super::Stage;
use super::kit::gap;
use nana_ui::runtime::view::{AnyView, El, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, BoxPaint, Button, ComponentView, LayoutBox, PaintContext, PaintPath, Painter, Stack,
    Text,
};
use nana_ui_core::{LengthSpec, OverflowSpec};
use nanatype_plugins::{DrawCommand, Mount, Node};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

struct Canvas {
    draw: Vec<DrawCommand>,
    key: u64,
}
impl Painter for Canvas {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        cx.save();
        cx.translate(bounds.x, bounds.y);
        for command in &self.draw {
            match command {
                DrawCommand::Rect {
                    rect,
                    color,
                    radius,
                } => {
                    cx.rounded_rect(
                        LayoutBox {
                            x: rect[0],
                            y: rect[1],
                            width: rect[2],
                            height: rect[3],
                        },
                        *radius,
                        BoxPaint::fill(color.map(|c| f32::from(c) / 255.)),
                    );
                }
                DrawCommand::Path {
                    points,
                    color,
                    closed,
                    width,
                } => {
                    let mut path = PaintPath::new();
                    if let Some(first) = points.first() {
                        path.move_to(first[0], first[1]);
                        for p in &points[1..] {
                            path.line_to(p[0], p[1]);
                        }
                    }
                    if *closed {
                        path.close();
                    }
                    let color = color.map(|c| f32::from(c) / 255.);
                    if *width > 0. {
                        cx.stroke_path(&path, *width, color);
                    } else {
                        cx.fill_path(&path, color);
                    }
                }
            }
        }
        cx.restore();
    }
    fn paint_key(&self) -> u64 {
        self.key
    }
}

fn draw_key(draw: &[DrawCommand]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for command in draw {
        std::mem::discriminant(command).hash(&mut hash);
        match command {
            DrawCommand::Rect {
                rect,
                color,
                radius,
            } => {
                rect.map(f32::to_bits).hash(&mut hash);
                color.hash(&mut hash);
                radius.to_bits().hash(&mut hash);
            }
            DrawCommand::Path {
                points,
                color,
                closed,
                width,
            } => {
                points.len().hash(&mut hash);
                for point in points {
                    point.map(f32::to_bits).hash(&mut hash);
                }
                color.hash(&mut hash);
                closed.hash(&mut hash);
                width.to_bits().hash(&mut hash);
            }
        }
    }
    hash.finish()
}

/// Sends a plugin command's event to wherever that plugin runs.
pub type Command<M> = Arc<dyn Fn(serde_json::Value) -> M + Send + Sync>;

/// Where a mounted widget's node sits in the latest updates: its package,
/// the mount, which of the package's views there, and the child indices
/// down from that view's root.
#[derive(Clone)]
struct Place {
    ui: Stage,
    package: String,
    slot: Mount,
    view: usize,
    path: Vec<usize>,
}

impl Place {
    fn child(&self, index: usize) -> Self {
        let mut child = self.clone();
        child.path.push(index);
        child
    }

    /// The node here now, tracked; `None` once it is gone.
    fn with<R>(&self, read: impl FnOnce(&Node) -> R) -> Option<R> {
        self.ui.plugin_views.with(|updates| {
            let view = updates
                .get(&self.package)?
                .views
                .iter()
                .filter(|view| view.slot == self.slot)
                .nth(self.view)?;
            let mut node = &view.root;
            for &index in &self.path {
                let (Node::Column { children, .. } | Node::Row { children, .. }) = node else {
                    return None;
                };
                node = children.get(index)?;
            }
            Some(read(node))
        })
    }
}

/// Keeps a mounted widget showing its node as updates of the same shape
/// arrive; one built once, for the debugger, stays as it is.
fn follow<C: ComponentView>(
    el: El<C>,
    place: Option<&Place>,
    mut write: impl FnMut(&mut C, &Node) + Send + 'static,
) -> El<C> {
    match place.cloned() {
        Some(place) => el.bind(move |c: &mut C| {
            place.with(|n| write(c, n));
        }),
        None => el,
    }
}

fn node<M: Send + 'static>(
    package: &str,
    n: &Node,
    command: &Command<M>,
    place: Option<&Place>,
) -> AnyView {
    match n {
        Node::Column { id, children } | Node::Row { id, children } => {
            let stack = if matches!(n, Node::Row { .. }) {
                Stack::row(0.0).gap_step(gap::step::TIGHT)
            } else {
                Stack::column(0.0).gap_step(gap::step::CONTROL)
            };
            widget(stack)
                .key(format!("plugin:{package}:{id}"))
                .children(
                    children
                        .iter()
                        .enumerate()
                        .map(|(i, child)| {
                            node(package, child, command, place.map(|p| p.child(i)).as_ref())
                        })
                        .collect::<Vec<_>>(),
                )
                .into_any()
        }
        Node::Text { id, text } => follow(
            widget(Text::new(text.clone())).key(format!("plugin:{package}:{id}")),
            place,
            |t: &mut Text, n| {
                if let Node::Text { text, .. } = n {
                    t.value.clone_from(text);
                }
            },
        )
        .into_any(),
        Node::Button {
            id,
            text,
            command: name,
        } => {
            let name = name.clone();
            let command = command.clone();
            follow(
                widget(Button::new(text.clone()))
                    .key(format!("plugin:{package}:{id}"))
                    .on_cx(move |_, _: &Activate, cx| {
                        cx.dispatch_program_all(command(
                            serde_json::json!({"type":"command","id":name}),
                        ));
                    }),
                place,
                |button: &mut Button, n| {
                    if let Node::Button { text, .. } = n {
                        button.label.clone_from(text);
                    }
                },
            )
            .into_any()
        }
        Node::Canvas {
            id,
            width,
            height,
            draw,
        } => {
            // The painter is swapped only for other commands: one with the
            // same key would be kept anyway, so the copy is skipped too.
            let mut shown = draw_key(draw);
            follow(
                widget(
                    Stack::column(0.)
                        .width(LengthSpec::Px(*width))
                        .height(LengthSpec::Px(*height))
                        .painter(Canvas {
                            draw: draw.clone(),
                            key: shown,
                        })
                        .with_layout(|l| {
                            l.overflow_x = OverflowSpec::Hidden;
                            l.overflow_y = OverflowSpec::Hidden;
                        }),
                )
                .key(format!("plugin:{package}:{id}")),
                place,
                move |stack: &mut Stack, n| {
                    if let Node::Canvas { draw, .. } = n {
                        let key = draw_key(draw);
                        if key != shown {
                            shown = key;
                            *stack = stack.clone().painter(Canvas {
                                draw: draw.clone(),
                                key,
                            });
                        }
                    }
                },
            )
            .into_any()
        }
    }
}

/// What a mount's views are built from, leaving out what they show: while
/// it holds, updates are written into the widgets already there.
#[derive(Clone, PartialEq)]
enum Shape {
    Column(String, Vec<Shape>),
    Row(String, Vec<Shape>),
    Text(String),
    Button { id: String, command: String },
    Canvas { id: String, width: f32, height: f32 },
}

impl Shape {
    fn of(n: &Node) -> Self {
        let children = |children: &[Node]| children.iter().map(Self::of).collect();
        match n {
            Node::Column { id, children: c } => Self::Column(id.clone(), children(c)),
            Node::Row { id, children: c } => Self::Row(id.clone(), children(c)),
            Node::Text { id, .. } => Self::Text(id.clone()),
            Node::Button { id, command, .. } => Self::Button {
                id: id.clone(),
                command: command.clone(),
            },
            Node::Canvas {
                id, width, height, ..
            } => Self::Canvas {
                id: id.clone(),
                width: *width,
                height: *height,
            },
        }
    }
}

/// The views running plugins mount at `slot`; `command(package)` routes a
/// widget's events back to that package's process.
pub fn mounted<M: Send + 'static>(
    ui: Stage,
    slot: Mount,
    command: impl Fn(String) -> Command<M> + Send + Sync + 'static,
) -> AnyView {
    let command = Arc::new(command);
    // Rebuilt only when this mount's shape changes: not on every update,
    // nor when another mount or another package changes.
    let shape = move || {
        ui.plugin_views.with(|updates| {
            let mut packages: Vec<(String, Vec<Shape>)> = updates
                .iter()
                .map(|(id, update)| {
                    let views: Vec<_> = update
                        .views
                        .iter()
                        .filter(|view| view.slot == slot)
                        .map(|view| Shape::of(&view.root))
                        .collect();
                    (id.clone(), views)
                })
                .filter(|(_, views)| !views.is_empty())
                .collect();
            packages.sort_by(|a, b| a.0.cmp(&b.0));
            packages
        })
    };
    dynamic(shape, move |packages| {
        let views = ui.plugin_views.with_untracked(|updates| {
            packages
                .iter()
                .filter_map(|(id, _)| updates.get_key_value(id))
                .flat_map(|(id, update)| {
                    let command = command(id.clone());
                    update
                        .views
                        .iter()
                        .filter(|view| view.slot == slot)
                        .enumerate()
                        .map(|(index, view)| {
                            let place = Place {
                                ui,
                                package: id.clone(),
                                slot,
                                view: index,
                                path: Vec::new(),
                            };
                            node(id, &view.root, &command, Some(&place))
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        });
        widget(Stack::column(0.)).children(views).into_any()
    })
    .into_any()
}

/// A script's views as the package editor's debugger shows them: the same
/// host widgets, but commands go back to the debug worker only.
pub fn debug_views<M: Send + 'static>(
    update: &nanatype_plugins::PluginUpdate,
    command: Command<M>,
) -> AnyView {
    let views = update
        .views
        .iter()
        .map(|view| {
            widget(Stack::column(0.0).gap_step(gap::step::TIGHT))
                .children((
                    widget(Text::new(format!("{:?}", view.slot).to_lowercase())),
                    node("debug", &view.root, &command, None),
                ))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
        .children(views)
        .into_any()
}

pub fn integrity_label(
    ui: Stage,
    status: &Result<Option<nanatype_theme::SealStatus>, String>,
) -> String {
    match status {
        Ok(Some(s)) => format!(
            "{} · {} · {} {}",
            ui.now_t("package-signature-valid"),
            fingerprint(&s.fingerprint),
            s.protected.len(),
            ui.now_t("package-protected-files")
        ),
        Ok(None) => ui.now_t("package-unsigned"),
        Err(error) => format!("{}: {error}", ui.now_t("package-integrity-failed")),
    }
}
pub fn fingerprint(hash: &str) -> String {
    hash.as_bytes()
        .chunks(8)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}
