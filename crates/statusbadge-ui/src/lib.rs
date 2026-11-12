use gpui::prelude::*;
use gpui::{
    div, px, rgb, rgba, AnyElement, Context, EventEmitter, IntoElement, MouseButton, Render,
    SharedString, Window,
};
use statusbadge_core::{combined, Status, Target, TargetState};

/// The badge window is a fixed square: adding a sudo never moves the centre on screen.
pub const BADGE_SIZE: f32 = 64.0;
const CENTER_DIAMETER: f32 = 24.0;
/// Production sudo: a bulb drawn behind the centre, only its outer part shows.
const BULB_DIAMETER: f32 = 18.0;
/// Staging or development sudo without a production sibling: a smaller bulb.
const LONE_DIAMETER: f32 = 14.0;
/// Staging or development sudo: an ear drawn behind its production bulb.
const EAR_DIAMETER: f32 = 10.0;
/// Distance between a bulb's centre and the centre circle's edge, as a fraction of the
/// bulb's radius: small enough to merge, large enough to leave a visible colour area.
const BULB_OFFSET: f32 = 0.3;
/// Ears sit on the bulb's edge, on each side of its outward direction.
const EAR_SPREAD_DEGREES: f32 = 55.0;
const EAR_DISTANCE: f32 = 1.05;
const WINDOW_MARGIN: f32 = 2.0;

/// Circle placement in window coordinates: left, top, diameter.
pub type Placement = (f32, f32, f32);

#[derive(Clone, Copy, Debug)]
pub struct Satellite {
    pub placement: Placement,
    /// Unit vector from the badge centre: the label sits on the visible side.
    pub outward: (f32, f32),
    pub ear: bool,
    /// This sudo is the centre circle itself (see `centre_item`).
    pub centre: bool,
}

/// Index of the sudo drawn as the centre circle: the only production sudo or, without
/// any production sudo, the only sudo. The centre has nothing to combine then, and a
/// bulb beside it would show the same status twice.
pub fn centre_item(items: &[(&str, bool)]) -> Option<usize> {
    let mut productions = (0..items.len()).filter(|&i| items[i].1);
    match (productions.next(), productions.next()) {
        (Some(only), None) => Some(only),
        (None, _) if items.len() == 1 => Some(0),
        _ => None,
    }
}

pub fn center_placement() -> Placement {
    let c = BADGE_SIZE / 2.0;
    (
        c - CENTER_DIAMETER / 2.0,
        c - CENTER_DIAMETER / 2.0,
        CENTER_DIAMETER,
    )
}

fn overlap(a: Placement, b: Placement) -> bool {
    let (ax, ay) = (a.0 + a.2 / 2.0, a.1 + a.2 / 2.0);
    let (bx, by) = (b.0 + b.2 / 2.0, b.1 + b.2 / 2.0);
    (ax - bx).hypot(ay - by) < (a.2 + b.2) / 2.0 + 0.5
}

/// One satellite per `(project, production)` item, in the given order (sorted by
/// `statusbadge_core::sort_targets`, so production comes first within a project).
///
/// Production sudos, and other environments without a production sibling, are bulbs
/// sharing the circle evenly from the bottom-right, clockwise. Other environments with a
/// production sibling are ears on that bulb. Every shape shrinks together until no two
/// projects touch and everything fits in the window.
///
/// The sudo chosen by `centre_item` takes the centre's placement instead of a bulb; its
/// other environments are ears of the centre and share the circle with the bulbs.
pub fn satellites(items: &[(&str, bool)]) -> Vec<Satellite> {
    let parent = |index: usize| {
        let (project, production) = items[index];
        (!production)
            .then(|| {
                items
                    .iter()
                    .position(|(other, prod)| *prod && other.eq_ignore_ascii_case(project))
            })
            .flatten()
    };
    let group: Vec<usize> = (0..items.len()).map(|i| parent(i).unwrap_or(i)).collect();
    let centre = centre_item(items);
    let centre_ear = |i: usize| Some(i) != centre && Some(group[i]) == centre;
    let slots: Vec<usize> = (0..items.len())
        .filter(|&i| Some(i) != centre && (group[i] == i || centre_ear(i)))
        .collect();
    let step = std::f32::consts::TAU / slots.len().max(1) as f32;
    let c = BADGE_SIZE / 2.0;
    let place = |scale: f32| {
        let mut shapes = vec![((0.0f32, 0.0f32, 0.0f32), (0.0f32, 0.0f32), false); items.len()];
        if let Some(i) = centre {
            shapes[i] = ((c, c, CENTER_DIAMETER), (0.0, 0.0), false);
        }
        for (slot, &i) in slots.iter().enumerate() {
            let angle = std::f32::consts::FRAC_PI_4 + step * slot as f32;
            let outward = (angle.cos(), angle.sin());
            let (diameter, distance) = if centre_ear(i) {
                (scale * EAR_DIAMETER, EAR_DISTANCE * CENTER_DIAMETER / 2.0)
            } else {
                let diameter = scale
                    * if items[i].1 {
                        BULB_DIAMETER
                    } else {
                        LONE_DIAMETER
                    };
                (
                    diameter,
                    CENTER_DIAMETER / 2.0 + BULB_OFFSET * diameter / 2.0,
                )
            };
            shapes[i] = (
                (c + distance * outward.0, c + distance * outward.1, diameter),
                outward,
                centre_ear(i),
            );
        }
        let mut ears = vec![0usize; items.len()];
        for i in 0..items.len() {
            let owner = group[i];
            if owner == i || Some(owner) == centre {
                continue;
            }
            let ((bx, by, bd), outward, _) = shapes[owner];
            let k = ears[owner];
            ears[owner] += 1;
            // First ear clockwise of the outward direction, then counter-clockwise.
            let side = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
            let spread = (EAR_SPREAD_DEGREES + (k / 2) as f32 * 45.0).to_radians();
            let angle = outward.1.atan2(outward.0) + side * spread;
            let distance = EAR_DISTANCE * bd / 2.0;
            shapes[i] = (
                (
                    bx + distance * angle.cos(),
                    by + distance * angle.sin(),
                    EAR_DIAMETER * scale,
                ),
                (angle.cos(), angle.sin()),
                true,
            );
        }
        shapes
            .into_iter()
            .enumerate()
            .map(|(i, ((x, y, d), outward, ear))| Satellite {
                placement: (x - d / 2.0, y - d / 2.0, d),
                outward,
                ear,
                centre: Some(i) == centre,
            })
            .collect::<Vec<_>>()
    };
    let fits = |shapes: &[Satellite]| {
        shapes.iter().enumerate().all(|(i, s)| {
            let p = s.placement;
            // Every bulb and ear merges with the centre by design.
            s.centre
                || p.0 >= WINDOW_MARGIN
                    && p.1 >= WINDOW_MARGIN
                    && p.0 + p.2 <= BADGE_SIZE - WINDOW_MARGIN
                    && p.1 + p.2 <= BADGE_SIZE - WINDOW_MARGIN
                    && shapes[..i].iter().enumerate().all(|(j, q)| {
                        // Shapes of one project merge by design; two ears of one bulb do not.
                        q.centre
                            || (group[i] == group[j] && !(s.ear && q.ear))
                            || !overlap(p, q.placement)
                    })
        })
    };
    let mut scale = 1.0;
    loop {
        let shapes = place(scale);
        if fits(&shapes) || scale <= 0.3 {
            return shapes;
        }
        scale -= 0.05;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    #[default]
    Idle,
    Available(String),
    Downloading,
    Ready,
    Failed,
    Current,
    Checking,
}
impl UpdateState {
    fn label(&self) -> &'static str {
        match self {
            Self::Available(_) => "↑ Mise à jour",
            Self::Downloading => "Téléchargement…",
            Self::Ready => "Glisser dans Applications",
            Self::Failed => "Réessayer",
            Self::Current => "À jour",
            Self::Checking => "Vérification…",
            Self::Idle => "",
        }
    }
}

#[derive(Clone, Default)]
pub struct Model {
    /// Sorted with `statusbadge_core::sort_targets`.
    pub targets: Vec<(Target, TargetState)>,
    pub update: UpdateState,
    pub now_ms: u64,
    pub reduce_motion: bool,
}
impl Model {
    pub fn target(&self, id: &str) -> Option<&(Target, TargetState)> {
        self.targets.iter().find(|(t, _)| t.id == id)
    }
    pub fn combined(&self) -> Option<(Status, u64)> {
        combined(self.targets.iter().map(|(t, s)| (t, s)), self.now_ms)
    }
    fn items(&self) -> Vec<(&str, bool)> {
        self.targets
            .iter()
            .map(|(t, _)| (t.name.as_str(), t.environment.is_production()))
            .collect()
    }
    /// The sudo shown by the centre circle itself, if any (see `centre_item`).
    pub fn centre_target(&self) -> Option<&(Target, TargetState)> {
        centre_item(&self.items()).map(|i| &self.targets[i])
    }
    /// Unlocked target with the most severe status, opened by a click on the centre.
    pub fn worst(&self) -> Option<&Target> {
        self.targets
            .iter()
            .filter(|(_, s)| !s.requires_login())
            .max_by_key(|(t, s)| {
                (
                    t.environment.is_production(),
                    s.indicator(self.now_ms).0.severity(),
                )
            })
            .map(|(t, _)| t)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Center,
    Target(String),
}

#[derive(Clone, PartialEq, Eq)]
pub enum Mode {
    Badge,
    Details(Cell),
    Update,
    Menu,
}

#[derive(Clone, Debug)]
pub enum Action {
    BeginDrag { cell: Cell, x: f32, y: f32 },
    DragAt { x: f32, y: f32 },
    EndDrag,
    Hover(Cell),
    HoverUpdate,
    Leave,
    Menu,
    Add,
    Remove(String),
    ResetPosition,
    CheckUpdates,
    InstallUpdate,
    Quit,
}

pub struct StatusView {
    pub model: Model,
    pub mode: Mode,
}
impl EventEmitter<Action> for StatusView {}

impl StatusView {
    pub fn new(model: Model, mode: Mode) -> Self {
        Self { model, mode }
    }
    pub fn set_model(&mut self, model: Model, cx: &mut Context<Self>) {
        self.model = model;
        cx.notify();
    }
    pub fn details_height(model: &Model, cell: &Cell) -> f32 {
        match cell {
            Cell::Center => 104.0 + (model.targets.len().max(1) as f32 * 27.0).min(340.0),
            Cell::Target(id) => {
                let Some((_, state)) = model.target(id) else {
                    return 120.0;
                };
                let rows = state.summary.as_ref().map_or(0, |s| s.sections.len());
                124.0
                    + (rows as f32 * 27.0).min(340.0)
                    + if state.message.is_some() { 40.0 } else { 0.0 }
            }
        }
    }
    pub fn menu_height(model: &Model) -> f32 {
        // Title, fixed entries and one entry per registered sudo.
        let fixed = 4;
        68.0 + 31.0 * (fixed + model.targets.len()) as f32
    }
    fn alarm_phase(&self) -> f64 {
        (1.0 - (self.model.now_ms as f64 / 2800.0 * std::f64::consts::TAU).cos()) / 2.0
    }
    fn alarm_color(&self) -> u32 {
        if self.model.reduce_motion {
            return Status::Outage.color();
        }
        // Pulse the cell itself as well as its halo so the alarm remains visible
        // against both light and dark desktops, without changing its size.
        let phase = self.alarm_phase();
        let red = (200.0 + 55.0 * phase) as u32;
        let green = (44.0 + 63.0 * phase) as u32;
        let blue = (61.0 + 54.0 * phase) as u32;
        (red << 16) | (green << 8) | blue
    }
    fn fill(&self, status: Status) -> u32 {
        if status == Status::Outage {
            self.alarm_color()
        } else {
            status.color()
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn circle(
        &self,
        id: SharedString,
        cell: Cell,
        (left, top, diameter): Placement,
        fill: Option<u32>,
        count: u64,
        // Shift of the label towards the visible part of a bulb.
        (dx, dy): (f32, f32),
        ear: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = count.to_string();
        let visible = if (dx, dy) == (0.0, 0.0) {
            diameter
        } else {
            diameter * 0.75
        };
        let font_size = ((visible - 4.0) / (label.len() as f32 * 0.65)).min(visible * 0.5);
        let hover_cell = cell.clone();
        div()
            .id(id)
            .absolute()
            .left(px(left))
            .top(px(top))
            .flex()
            .items_center()
            .justify_center()
            .size(px(diameter))
            .rounded_full()
            .border_1()
            .border_color(rgba(0x13191ddd))
            .bg(match fill {
                Some(color) => rgb(color),
                None => rgb(0x303944),
            })
            .cursor_pointer()
            .hover(|s| s.border_color(rgba(0xffffff88)))
            .on_hover(cx.listener(move |_, over: &bool, _, cx| {
                cx.emit(if *over {
                    Action::Hover(hover_cell.clone())
                } else {
                    Action::Leave
                });
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |_, event: &gpui::MouseDownEvent, _, cx| {
                    cx.emit(Action::BeginDrag {
                        cell: cell.clone(),
                        x: event.position.x.as_f32(),
                        y: event.position.y.as_f32(),
                    })
                }),
            )
            // Ears are partly hidden by their bulb: colour only, no icon or count.
            .when(fill.is_none() && !ear && diameter >= 8.0, |s| {
                s.child(
                    div()
                        .absolute()
                        .left(px(dx))
                        .top(px(dy))
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(lock_icon(visible)),
                )
            })
            .when(
                fill.is_some() && count > 0 && !ear && diameter >= 10.0,
                |s| {
                    let text = |shadow: bool| {
                        div()
                            .absolute()
                            .left(px(dx + if shadow { 0.5 } else { 0.0 }))
                            .top(px(dy + if shadow { 0.75 } else { 0.0 }))
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(font_size))
                            .line_height(px(diameter - 2.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if shadow {
                                rgba(0x14202dcc)
                            } else {
                                rgb(0xffffff)
                            })
                            .child(label.clone())
                    };
                    s.child(
                        div()
                            .relative()
                            .size_full()
                            .child(text(true))
                            .child(text(false)),
                    )
                },
            )
            .into_any_element()
    }
    fn halo(&self, (left, top, diameter): Placement) -> AnyElement {
        let margin = (diameter / 4.0).clamp(3.0, 5.0);
        let alpha = if self.model.reduce_motion {
            64
        } else {
            (18.0 + 110.0 * self.alarm_phase()) as u32
        };
        div()
            .absolute()
            .left(px(left - margin))
            .top(px(top - margin))
            .size(px(diameter + 2.0 * margin))
            .rounded_full()
            .bg(rgba(0xff5b6400 | alpha))
            .into_any_element()
    }
    fn badge(&self, cx: &mut Context<Self>) -> AnyElement {
        let now = self.model.now_ms;
        let shapes = satellites(&self.model.items());
        let center = center_placement();
        // With a sudo as the centre, its own halo below replaces the combined one.
        let sole = self.model.centre_target();
        let combined = self.model.combined().filter(|_| sole.is_none());
        let mut root = div()
            .id("statusbadge")
            .relative()
            .size(px(BADGE_SIZE))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.emit(Action::Menu)),
            );
        // Halos first so they stay under every shape.
        if combined.is_some_and(|(s, _)| s == Status::Outage) {
            root = root.child(self.halo(center));
        }
        for ((_, state), shape) in self.model.targets.iter().zip(&shapes) {
            if !state.requires_login() && state.indicator(now).0 == Status::Outage {
                root = root.child(self.halo(shape.placement));
            }
        }
        // Ears under bulbs, bulbs under the centre: each shows only its outer part.
        for ears in [true, false] {
            for ((target, state), shape) in self.model.targets.iter().zip(&shapes) {
                if shape.ear != ears || shape.centre {
                    continue;
                }
                let (status, count) = state.indicator(now);
                let shift = if ears {
                    (0.0, 0.0)
                } else {
                    let r = shape.placement.2 / 2.0;
                    (shape.outward.0 * r * 0.4, shape.outward.1 * r * 0.4)
                };
                root = root.child(self.circle(
                    SharedString::from(format!("target-{}", target.id)),
                    Cell::Target(target.id.clone()),
                    shape.placement,
                    (!state.requires_login()).then(|| self.fill(status)),
                    count,
                    shift,
                    shape.ear,
                    cx,
                ));
            }
        }
        root = match (sole, combined) {
            // The centre is this sudo: its status, count, padlock, details and click.
            (Some((target, state)), _) => {
                let (status, count) = state.indicator(now);
                root.child(self.circle(
                    "center".into(),
                    Cell::Target(target.id.clone()),
                    center,
                    (!state.requires_login()).then(|| self.fill(status)),
                    count,
                    (0.0, 0.0),
                    false,
                    cx,
                ))
            }
            (None, Some((status, count))) => root.child(self.circle(
                "center".into(),
                Cell::Center,
                center,
                Some(self.fill(status)),
                count,
                (0.0, 0.0),
                false,
                cx,
            )),
            (None, None) => root.child(self.empty_center(center, cx)),
        };
        if !matches!(self.model.update, UpdateState::Idle) {
            root = root.child(self.update_button(cx));
        }
        root.into_any_element()
    }
    fn empty_center(&self, (left, top, diameter): Placement, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("center")
            .absolute()
            .left(px(left))
            .top(px(top))
            .size(px(diameter))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(rgba(0x13191ddd))
            .bg(rgb(0x303944))
            .text_color(rgb(0xc5ced8))
            .text_size(px(17.0))
            .line_height(px(diameter - 2.0))
            .cursor_pointer()
            .hover(|s| s.border_color(rgba(0xffffff88)))
            .on_hover(cx.listener(|_, over: &bool, _, cx| {
                cx.emit(if *over {
                    Action::Hover(Cell::Center)
                } else {
                    Action::Leave
                })
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, event: &gpui::MouseDownEvent, _, cx| {
                    cx.emit(Action::BeginDrag {
                        cell: Cell::Center,
                        x: event.position.x.as_f32(),
                        y: event.position.y.as_f32(),
                    })
                }),
            )
            .child("+")
            .into_any_element()
    }
    fn update_button(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("update")
            .absolute()
            .left(px(BADGE_SIZE - 17.0))
            .top(px(2.0))
            .size(px(15.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(rgb(0x273848))
            .bg(rgb(0x9ac8ef))
            .text_color(rgb(0x182a3c))
            .text_size(px(12.0))
            .cursor_pointer()
            .on_hover(cx.listener(|_, over: &bool, _, cx| {
                cx.emit(if *over {
                    Action::HoverUpdate
                } else {
                    Action::Leave
                })
            }))
            .on_click(cx.listener(|this, _, _, cx| {
                if matches!(
                    this.model.update,
                    UpdateState::Available(_) | UpdateState::Failed
                ) {
                    cx.emit(Action::InstallUpdate);
                }
            }))
            .child(match self.model.update {
                UpdateState::Downloading | UpdateState::Checking => "…",
                UpdateState::Current | UpdateState::Ready => "✓",
                UpdateState::Failed => "!",
                _ => "↑",
            })
            .into_any_element()
    }
    fn drag_capture(&self, cx: &mut Context<Self>) -> AnyElement {
        let moving = cx.listener(|_, event: &gpui::MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                cx.emit(Action::DragAt {
                    x: event.position.x.as_f32(),
                    y: event.position.y.as_f32(),
                });
            }
        });
        let released = cx.listener(|_, _: &gpui::MouseUpEvent, _, cx| cx.emit(Action::EndDrag));
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Bubble {
                        moving(event, window, cx);
                    }
                });
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Bubble && event.button == MouseButton::Left {
                        released(event, window, cx);
                    }
                });
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }
    fn update_details(&self) -> AnyElement {
        panel()
            .gap(px(8.0))
            .child(self.model.update.label())
            .when_some(
                match self.model.update {
                    UpdateState::Available(ref version) => Some(format!(
                        "Version {version} · cliquer sur la flèche pour télécharger le DMG."
                    )),
                    UpdateState::Ready => Some(
                        "DMG ouvert. Après le remplacement dans Applications, le badge redémarre en moins d’une minute."
                            .to_string(),
                    ),
                    _ => None,
                },
                |s, detail| {
                    s.child(
                        div()
                            .text_color(rgb(0x98a6b5))
                            .text_size(px(10.0))
                            .child(detail),
                    )
                },
            )
            .into_any_element()
    }
    fn overview(&self) -> AnyElement {
        let now = self.model.now_ms;
        let header = match self.model.combined() {
            Some((status, count)) => (
                status.color(),
                if count > 0 {
                    format!("{count} alerte{}", if count > 1 { "s" } else { "" })
                } else {
                    status.label().into()
                },
            ),
            None => (Status::Unknown.color(), "Aucun sudo".into()),
        };
        panel()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child("Status Badge")
                    .child(div().text_color(rgb(header.0)).child(header.1)),
            )
            .child(div().text_size(px(10.0)).text_color(rgb(0x98a6b5)).child(
                if self.model.targets.is_empty() {
                    "Cliquer pour ajouter l’URL d’une interface sudo"
                } else {
                    "Centre : environnements de production · actualisation toutes les 3 secondes"
                },
            ))
            .child(
                div()
                    .id("targets")
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .max_h(px(340.0))
                    .overflow_y_scroll()
                    .children(self.model.targets.iter().map(|(target, state)| {
                        let (status, count) = state.indicator(now);
                        let text = if state.requires_login() {
                            "Connexion requise".to_string()
                        } else if count > 0 {
                            count.to_string()
                        } else {
                            status.label().into()
                        };
                        row(
                            target.title(),
                            text,
                            if state.requires_login() {
                                Status::Unknown
                            } else {
                                status
                            },
                            !target.environment.is_production(),
                        )
                    })),
            )
            .child(footer(if self.model.targets.is_empty() {
                "Clic : ajouter · Glisser : déplacer · Clic droit : options"
            } else {
                "Clic : sudo le plus grave · Glisser : déplacer · Clic droit : options"
            }))
            .into_any_element()
    }
    fn details(&self, id: &str) -> AnyElement {
        let Some((target, state)) = self.model.target(id) else {
            return panel().child("Sudo retiré").into_any_element();
        };
        let now = self.model.now_ms;
        let rows: Vec<(String, Status, u64)> = state
            .summary
            .iter()
            .flat_map(|s| &s.sections)
            .map(|section| (section.label.clone(), section.status, section.count))
            .collect();
        let stale = state.summary.as_ref().is_some_and(|s| s.stale(now));
        let subtitle = if state.requires_login() {
            "Cliquer sur le cadenas pour se connecter".to_string()
        } else if state.offline {
            "Connexion indisponible · dernières données reçues".into()
        } else if stale {
            "Données périmées · plus de 30 secondes".into()
        } else {
            format!(
                "{} · actualisation toutes les 3 secondes",
                target.environment.label()
            )
        };
        let (status, count) = state.indicator(now);
        panel()
            .gap(px(10.0))
            .child(
                div().flex().justify_between().child(target.title()).child(
                    div()
                        .text_color(rgb(status.color()))
                        .child(if state.summary.is_none() {
                            if state.requires_login() {
                                "Connexion requise"
                            } else {
                                "Chargement…"
                            }
                            .to_string()
                        } else if count > 0 {
                            format!("{count} alerte{}", if count > 1 { "s" } else { "" })
                        } else {
                            status.label().into()
                        }),
                ),
            )
            .child(
                div()
                    .text_size(px(10.0))
                    .text_color(rgb(0x98a6b5))
                    .child(subtitle),
            )
            .child(
                div()
                    .id("status-rows")
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .max_h(px(340.0))
                    .overflow_y_scroll()
                    .children(rows.into_iter().map(|(label, status, count)| {
                        row(
                            label,
                            if count > 0 {
                                count.to_string()
                            } else {
                                status.label().into()
                            },
                            status,
                            false,
                        )
                    })),
            )
            .when(state.message.is_some(), |s| {
                s.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(0xd0af81))
                        .child(state.message.clone().unwrap_or_default()),
                )
            })
            .child(footer(
                "Clic : administration · Glisser : déplacer · Clic droit : options",
            ))
            .into_any_element()
    }
    fn menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut items = vec![("Ajouter un sudo…".to_string(), Action::Add)];
        items.extend(self.model.targets.iter().map(|(target, _)| {
            (
                format!("Retirer {}", target.title()),
                Action::Remove(target.id.clone()),
            )
        }));
        items.extend([
            (
                "Rechercher une mise à jour".to_string(),
                Action::CheckUpdates,
            ),
            ("Replacer le badge".to_string(), Action::ResetPosition),
            ("Quitter".to_string(), Action::Quit),
        ]);
        panel()
            .gap(px(3.0))
            .child(
                div()
                    .mb(px(5.0))
                    .text_color(rgb(0x82909f))
                    .child(format!("Status Badge {}", env!("CARGO_PKG_VERSION"))),
            )
            .children(items.into_iter().enumerate().map(|(i, (label, action))| {
                div()
                    .id(i)
                    .px(px(8.0))
                    .py(px(7.0))
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .truncate()
                    .hover(|s| s.bg(rgb(0x303843)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(action.clone())))
                    .child(label)
            }))
            .into_any_element()
    }
}

impl Render for StatusView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .font_family(".AppleSystemUIFont")
            .text_size(px(11.0))
            .text_color(rgb(0xe0e5ec))
            .when(self.mode == Mode::Badge, |root| {
                root.child(self.drag_capture(cx))
            })
            .child(match &self.mode {
                Mode::Badge => self.badge(cx),
                Mode::Details(Cell::Center) => self.overview(),
                Mode::Details(Cell::Target(id)) => self.details(id),
                Mode::Update => self.update_details(),
                Mode::Menu => self.menu(cx),
            })
    }
}
fn panel() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .p(px(13.0))
        .rounded(px(11.0))
        .border_1()
        .border_color(rgb(0x363c45))
        .bg(rgba(0x181c22fc))
}
fn footer(text: &'static str) -> gpui::Div {
    div()
        .border_t_1()
        .border_color(rgb(0x303843))
        .pt(px(9.0))
        .text_size(px(10.0))
        .text_color(rgb(0x82909f))
        .child(text)
}
fn row(label: String, value: String, status: Status, secondary: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(17.0))
        .flex_shrink_0()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(rgb(if secondary { 0x82909f } else { 0xaeb8c5 }))
                .child(label),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(status.color()))
                        .child(value),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .size(px(6.0))
                        .rounded_full()
                        .bg(rgb(status.color())),
                ),
        )
}
fn lock_icon(diameter: f32) -> AnyElement {
    // A monochrome lock, avoiding the coloured system emoji glyph. Scaled to the circle.
    let k = (diameter / 16.0).min(1.0);
    div()
        .relative()
        .w(px(9.0 * k))
        .h(px(11.0 * k))
        .child(
            div()
                .absolute()
                .left(px(2.0 * k))
                .top(px(0.0))
                .w(px(5.0 * k))
                .h(px(7.0 * k))
                .rounded_t(px(3.0 * k))
                .border_1()
                .border_color(rgb(0x98a6b5)),
        )
        .child(
            div()
                .absolute()
                .left(px(0.0))
                .top(px(4.0 * k))
                .w(px(9.0 * k))
                .h(px(7.0 * k))
                .rounded(px(2.0 * k))
                .bg(rgb(0x98a6b5)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn centre(p: Placement) -> (f32, f32) {
        (p.0 + p.2 / 2.0, p.1 + p.2 / 2.0)
    }
    fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
        (a.0 - b.0).hypot(a.1 - b.1)
    }
    #[test]
    fn projects_stay_inside_the_window_and_never_touch_each_other() {
        // Beyond eight projects shapes reach their minimum size and may touch.
        let names = ["a", "b", "c", "d", "e", "f", "g", "h"];
        for n in 1..=8 {
            for pattern in 0..4 {
                let mut items = Vec::new();
                for (index, name) in names[..n].iter().enumerate() {
                    // Mixes lone production, production with one or two ears, lone staging.
                    match (pattern + index) % 4 {
                        0 => items.push((*name, true)),
                        1 => items.extend([(*name, true), (*name, false)]),
                        2 => items.extend([(*name, true), (*name, false), (*name, false)]),
                        _ => items.push((*name, false)),
                    }
                }
                let shapes = satellites(&items);
                assert_eq!(shapes.len(), items.len());
                let productions = items.iter().filter(|(_, production)| *production).count();
                assert_eq!(
                    shapes.iter().filter(|s| s.centre).count(),
                    usize::from(productions == 1 || items.len() == 1),
                    "{n} {pattern}"
                );
                for (i, s) in shapes.iter().enumerate() {
                    let p = s.placement;
                    if s.centre {
                        assert_eq!(p, center_placement());
                        continue;
                    }
                    assert!(
                        p.0 >= WINDOW_MARGIN && p.1 >= WINDOW_MARGIN,
                        "{n} {i} {p:?}"
                    );
                    assert!(p.0 + p.2 <= BADGE_SIZE - WINDOW_MARGIN);
                    assert!(p.1 + p.2 <= BADGE_SIZE - WINDOW_MARGIN);
                    for (j, q) in shapes[..i].iter().enumerate() {
                        if q.centre {
                            continue;
                        }
                        if items[i].0 != items[j].0 || (s.ear && q.ear) {
                            assert!(!overlap(p, q.placement), "{n} {pattern} {i} {j}");
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn bulbs_merge_with_the_centre_and_ears_with_their_bulb() {
        let shapes = satellites(&[
            ("atlas", true),
            ("delta", true),
            ("delta", false),
            ("cobalt", false),
        ]);
        let c = (BADGE_SIZE / 2.0, BADGE_SIZE / 2.0);
        let (bulb, ear, lone) = (shapes[1], shapes[2], shapes[3]);
        assert!(shapes.iter().all(|s| !s.centre));
        assert!(!bulb.ear && ear.ear && !lone.ear);
        // Each bulb overlaps the centre circle but sticks out of it.
        for s in [bulb, lone] {
            let d = distance(centre(s.placement), c);
            assert!(d < (CENTER_DIAMETER + s.placement.2) / 2.0);
            assert!(d + s.placement.2 / 2.0 > CENTER_DIAMETER / 2.0 + 4.0);
        }
        assert!(lone.placement.2 < bulb.placement.2);
        // The ear overlaps its bulb, further out than the bulb's centre.
        let d = distance(centre(ear.placement), centre(bulb.placement));
        assert!(d < (bulb.placement.2 + ear.placement.2) / 2.0);
        assert!(distance(centre(ear.placement), c) > distance(centre(bulb.placement), c));
    }
    #[test]
    fn a_single_production_sudo_is_the_centre_itself() {
        let shapes = satellites(&[("delta", true), ("delta", false), ("cobalt", false)]);
        let c = (BADGE_SIZE / 2.0, BADGE_SIZE / 2.0);
        let (prod, ear, lone) = (shapes[0], shapes[1], shapes[2]);
        assert!(prod.centre && !ear.centre && !lone.centre);
        assert_eq!(prod.placement, center_placement());
        // Its staging is an ear of the centre: behind it, sticking out.
        assert!(ear.ear && !lone.ear);
        let d = distance(centre(ear.placement), c);
        assert!(d < (CENTER_DIAMETER + ear.placement.2) / 2.0);
        assert!(d + ear.placement.2 / 2.0 > CENTER_DIAMETER / 2.0 + 4.0);
        // Another project without production stays a bulb, apart from that ear.
        assert_eq!(lone.placement.2, LONE_DIAMETER);
        assert!(!overlap(ear.placement, lone.placement));
    }
    #[test]
    fn the_centre_takes_a_sudo_only_when_it_has_nothing_to_combine() {
        // One production sudo, whatever surrounds it.
        assert_eq!(centre_item(&[("a", true)]), Some(0));
        assert_eq!(
            centre_item(&[("a", false), ("b", true), ("b", false)]),
            Some(1)
        );
        // A single sudo of any environment.
        assert_eq!(centre_item(&[("a", false)]), Some(0));
        // Several productions, or several sudos without production: combined centre.
        assert_eq!(centre_item(&[("a", true), ("b", true)]), None);
        assert_eq!(centre_item(&[("a", false), ("b", false)]), None);
        assert_eq!(centre_item(&[]), None);
        assert!(satellites(&[]).is_empty());
        let alone = satellites(&[("a", true)]);
        assert!(alone.len() == 1 && alone[0].centre);
    }
}
