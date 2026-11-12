mod config;
mod native;
mod prompt;
mod updater;
mod worker;

use config::Config;
use crossbeam_channel::{Receiver, Sender};
use gpui::{
    point, px, size, App, AppContext, AsyncApp, Bounds, Entity, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use rand::RngCore;
use statusbadge_core::{
    auth::{normalize_base, parse_callback, PendingLogin},
    sort_targets,
    updates::Release,
    Status, Summary, Target, TargetEnvironment, TargetState,
};
use statusbadge_ui::{Action, Cell, Mode, Model, StatusView, UpdateState, BADGE_SIZE};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

struct Panel {
    handle: WindowHandle<StatusView>,
    native: native::NativeWindow,
    view: Entity<StatusView>,
    mode: Mode,
}
struct DragGesture {
    cell: Cell,
    anchor: [f64; 2],
    moved: bool,
}
/// A registered sudo, or one being connected (`saved == false`) until its first summary.
struct Entry {
    target: Target,
    state: TargetState,
    worker: Option<Sender<worker::Command>>,
    saved: bool,
}
struct State {
    config: Config,
    model: Model,
    entries: Vec<Entry>,
    pending: Vec<PendingLogin>,
    worker_events: Sender<worker::Event>,
    prompts: Sender<String>,
    prompting: bool,
    badge: Panel,
    popover: Option<Panel>,
    last_inside: Instant,
    last_position: Option<[f64; 2]>,
    position_changed: Option<Instant>,
    update: Option<Release>,
    next_update_check: Instant,
    checking_update: bool,
    updating: bool,
    /// End of the display of a manual check's answer ("À jour", or a failed check).
    update_notice_until: Option<Instant>,
    /// Installed bundle watched for a replacement by a newer version.
    bundle: Option<std::path::PathBuf>,
    next_bundle_check: Instant,
    demo: bool,
    drag: Option<DragGesture>,
}
type SharedState = Rc<RefCell<State>>;
/// How long the answer of a manual update check stays on the badge.
const NOTICE: Duration = Duration::from_secs(6);
enum UpdateEvent {
    Checked(Result<Option<Release>, ()>, bool),
    Downloaded(bool),
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            if std::env::var("STATUSBADGE_DEBUG_LAYOUT").as_deref() == Ok("1") {
                "statusbadge=debug"
            } else {
                "statusbadge=info"
            },
        )
        .init();
    let demo = std::env::args().any(|arg| arg == "--demo");
    let _instance = if demo {
        None
    } else {
        Some(config::single_instance()?)
    };
    let config = if demo {
        Config {
            targets: demo_targets(),
            ..Default::default()
        }
    } else {
        Config::load()
    };
    let (events_tx, events) = crossbeam_channel::unbounded();
    let entries: Vec<Entry> = config
        .targets
        .iter()
        .map(|target| Entry {
            target: target.clone(),
            state: TargetState::default(),
            worker: (!demo).then(|| worker::spawn(target.clone(), None, events_tx.clone())),
            saved: true,
        })
        .collect();
    let (updates_tx, updates) = crossbeam_channel::unbounded();
    let (urls_tx, urls) = crossbeam_channel::unbounded();
    let (prompts_tx, prompts) = crossbeam_channel::unbounded();
    let app = gpui_platform::application();
    app.on_open_urls(move |incoming| {
        for url in incoming {
            let _ = urls_tx.send(url);
        }
    });
    app.run(move |cx| {
        native::accessory_application();
        let model = Model {
            now_ms: worker::now_ms(),
            ..Default::default()
        };
        let badge = open_panel(Mode::Badge, model.clone(), cx).expect("cannot create overlay");
        native::place_badge(config.position);
        let state = Rc::new(RefCell::new(State {
            config,
            model,
            entries,
            pending: Vec::new(),
            worker_events: events_tx,
            prompts: prompts_tx,
            prompting: false,
            badge,
            popover: None,
            last_inside: Instant::now(),
            last_position: native::position(),
            position_changed: None,
            update: None,
            next_update_check: Instant::now(),
            checking_update: false,
            updating: false,
            update_notice_until: None,
            // A bundle already newer than this executable at launch is a packaging
            // error, not an update: watching it would restart in a loop.
            bundle: updater::running_bundle()
                .filter(|bundle| !demo && !updater::bundle_was_updated(bundle)),
            next_bundle_check: Instant::now(),
            demo,
            drag: None,
        }));
        {
            let mut state = state.borrow_mut();
            if demo {
                load_demo(&mut state);
            }
            refresh(&mut state, cx);
        }
        subscribe(
            &state,
            state.borrow().badge.view.clone(),
            updates_tx.clone(),
            cx,
        );
        tick(state, events, urls, prompts, updates, updates_tx, cx);
    });
    Ok(())
}

fn demo_targets() -> Vec<Target> {
    let target = |name: &str, environment| Target {
        id: format!("demo-{name}-{environment:?}").to_lowercase(),
        base: format!("https://sudo.{}.test", name.to_lowercase()),
        name: name.into(),
        environment,
        summary: "https://demo.invalid/summary".into(),
        open: "https://demo.invalid".into(),
    };
    let mut targets = vec![
        target("Delta", TargetEnvironment::Production),
        target("Delta", TargetEnvironment::Staging),
        target("Atlas", TargetEnvironment::Production),
        target("Cobalt", TargetEnvironment::Production),
        target("Cobalt", TargetEnvironment::Staging),
        target("Boreal", TargetEnvironment::Production),
        target("Boreal", TargetEnvironment::Development),
    ];
    sort_targets(&mut targets);
    targets
}
/// Demo data only: no Keychain access and no network call.
fn load_demo(state: &mut State) {
    let summary: Summary =
        serde_json::from_str(include_str!("../../../fixtures/monitoring-summary.json"))
            .expect("valid demo summary");
    for entry in &mut state.entries {
        let mut summary = summary.clone();
        (summary.status, summary.count) =
            match (entry.target.name.as_str(), entry.target.environment) {
                ("Delta", TargetEnvironment::Production) => (Status::Degraded, 3),
                ("Delta", _) => (Status::Outage, 1),
                ("Cobalt", TargetEnvironment::Production) => (Status::Watch, 0),
                ("Boreal", TargetEnvironment::Development) => (Status::Unknown, 0),
                _ => (Status::Operational, 0),
            };
        entry.state = if entry.target.name == "Cobalt" && !entry.target.environment.is_production()
        {
            TargetState::locked("Connexion requise")
        } else {
            TargetState {
                authenticated: true,
                summary: Some(summary),
                ..Default::default()
            }
        };
    }
    state.model.update = UpdateState::Available("0.2.0 · aperçu".into());
}

fn open_panel(mode: Mode, model: Model, cx: &mut App) -> anyhow::Result<Panel> {
    let (width, height, title) = match &mode {
        Mode::Badge => (BADGE_SIZE, BADGE_SIZE, native::BADGE),
        Mode::Update => (280.0, 100.0, native::POPOVER),
        Mode::Details(cell) => (
            350.0,
            StatusView::details_height(&model, cell),
            native::POPOVER,
        ),
        Mode::Menu => (280.0, StatusView::menu_height(&model), native::POPOVER),
    };
    let is_badge = mode == Mode::Badge;
    let handle = cx.open_window(
        WindowOptions {
            titlebar: None,
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(24.0), px(100.0)),
                size: size(px(width), px(height)),
            })),
            kind: WindowKind::PopUp,
            is_movable: is_badge,
            is_resizable: false,
            is_minimizable: false,
            focus: false,
            show: is_badge,
            window_background: WindowBackgroundAppearance::Transparent,
            app_id: Some("fr.gmbl.statusbadge".into()),
            ..Default::default()
        },
        |window, cx| {
            window.set_window_title(title);
            cx.new(|_| StatusView::new(model, mode.clone()))
        },
    )?;
    let view = handle.entity(cx)?;
    let native = handle.update(cx, |_, window, _| native::from_gpui(window))??;
    native::configure(&native, is_badge);
    if !is_badge {
        native::anchor_popover(&native, width as f64, height as f64);
        native::show(&native, mode == Mode::Menu);
    }
    Ok(Panel {
        handle,
        native,
        view,
        mode,
    })
}
fn close_popover(state: &mut State, cx: &mut App) {
    if let Some(panel) = state.popover.take() {
        native::hide(&panel.native);
        let _ = panel
            .handle
            .update(cx, |_, window, _| window.remove_window());
    }
}
fn open_popover(
    shared: &SharedState,
    state: &mut State,
    mode: Mode,
    updates: Sender<UpdateEvent>,
    cx: &mut App,
) {
    close_popover(state, cx);
    if let Ok(panel) = open_panel(mode, state.model.clone(), cx) {
        subscribe(shared, panel.view.clone(), updates, cx);
        state.popover = Some(panel);
    }
}
fn refresh(state: &mut State, cx: &mut App) {
    state.model.targets = state
        .entries
        .iter()
        .map(|e| (e.target.clone(), e.state.clone()))
        .collect();
    state
        .badge
        .view
        .update(cx, |view, cx| view.set_model(state.model.clone(), cx));
    if let Some(panel) = &state.popover {
        if let Mode::Details(cell) = &panel.mode {
            native::anchor_popover(
                &panel.native,
                350.0,
                StatusView::details_height(&state.model, cell) as f64,
            );
        }
        panel
            .view
            .update(cx, |view, cx| view.set_model(state.model.clone(), cx));
    }
}
fn subscribe(
    state: &SharedState,
    view: Entity<StatusView>,
    updates: Sender<UpdateEvent>,
    cx: &mut App,
) {
    let state = state.clone();
    cx.subscribe(&view, move |_, action, cx| {
        handle_action(&state, action.clone(), updates.clone(), cx)
    })
    .detach();
}
/// Opens the sudo's `/statusbadge` route. The pending request is kept in memory only.
fn begin_login(state: &mut State, base: String, target: Option<String>, cx: &mut App) {
    let mut random = [0u8; 32];
    rand::rng().fill_bytes(&mut random);
    let now = worker::now_ms();
    state
        .pending
        .retain(|p| now.saturating_sub(p.created_at) <= statusbadge_core::auth::LOGIN_TTL_MS);
    // One request per sudo: a new click replaces the previous one.
    state.pending.retain(|p| p.base != base);
    let request = PendingLogin {
        base,
        target,
        state: random.iter().map(|b| format!("{b:02x}")).collect(),
        created_at: now,
    };
    cx.open_url(&request.login_url());
    state.pending.push(request);
}
fn ask_for_sudo(state: &mut State, message: &'static str) {
    if state.prompting || state.demo {
        return;
    }
    state.prompting = true;
    let prompts = state.prompts.clone();
    std::thread::spawn(move || {
        // An empty string reports a cancelled dialog.
        let _ = prompts.send(prompt::ask_url(message).unwrap_or_default());
    });
}
const ASK: &str = "URL HTTPS de l’interface d’administration (par exemple https://sudo.example.com). Le navigateur s’ouvre ensuite pour la connexion.";

fn handle_action(shared: &SharedState, action: Action, updates: Sender<UpdateEvent>, cx: &mut App) {
    let mut state = shared.borrow_mut();
    if state.demo && !matches!(action, Action::DragAt { .. }) {
        tracing::info!(?action, "demo interaction");
    }
    match action {
        Action::Hover(cell) => {
            if state.drag.is_some() {
                return;
            }
            state.last_inside = Instant::now();
            let target = Mode::Details(cell);
            if state
                .popover
                .as_ref()
                .is_some_and(|p| p.mode == Mode::Menu || p.mode == target)
            {
                return;
            }
            open_popover(shared, &mut state, target, updates, cx);
        }
        Action::HoverUpdate => {
            if state.drag.is_some() {
                return;
            }
            state.last_inside = Instant::now();
            if state
                .popover
                .as_ref()
                .is_some_and(|p| p.mode == Mode::Menu || p.mode == Mode::Update)
            {
                return;
            }
            open_popover(shared, &mut state, Mode::Update, updates, cx);
        }
        Action::Leave => {
            state.last_inside = Instant::now();
        }
        Action::Menu => open_popover(shared, &mut state, Mode::Menu, updates, cx),
        Action::BeginDrag { cell, x, y } => {
            close_popover(&mut state, cx);
            state.drag = Some(DragGesture {
                cell,
                anchor: [x as f64, y as f64],
                moved: false,
            });
        }
        Action::DragAt { x, y } => {
            if let Some(drag) = &mut state.drag {
                let delta = [x as f64 - drag.anchor[0], drag.anchor[1] - y as f64];
                drag.moved |= delta[0].hypot(delta[1]) > 4.0;
                if drag.moved {
                    if let Some(origin) = native::position() {
                        native::move_badge([origin[0] + delta[0], origin[1] + delta[1]]);
                    }
                }
            }
        }
        Action::EndDrag => {
            let Some(drag) = state.drag.take() else {
                return;
            };
            if drag.moved {
                return;
            }
            match drag.cell {
                Cell::Center => match state.model.worst() {
                    Some(target) => cx.open_url(&target.open),
                    None if state.entries.is_empty() => ask_for_sudo(&mut state, ASK),
                    None => {}
                },
                Cell::Target(id) => {
                    let Some(entry) = state.entries.iter().find(|e| e.target.id == id) else {
                        return;
                    };
                    if !entry.state.requires_login() {
                        cx.open_url(&entry.target.open);
                    } else if !state.demo {
                        let base = entry.target.base.clone();
                        begin_login(&mut state, base, Some(id), cx);
                    }
                }
            }
        }
        Action::Add => {
            close_popover(&mut state, cx);
            ask_for_sudo(&mut state, ASK);
        }
        Action::Remove(id) => {
            if let Some(index) = state.entries.iter().position(|e| e.target.id == id) {
                let entry = state.entries.remove(index);
                match entry.worker {
                    Some(worker) => {
                        let _ = worker.send(worker::Command::Remove);
                    }
                    None if !state.demo => {
                        worker::remove_session(&id);
                    }
                    None => {}
                }
                state.config.targets.retain(|t| t.id != id);
                state.pending.retain(|p| p.target.as_deref() != Some(&id));
                save(&state);
            }
            close_popover(&mut state, cx);
            refresh(&mut state, cx);
        }
        Action::ResetPosition => {
            native::place_badge(None);
            close_popover(&mut state, cx);
        }
        Action::CheckUpdates => {
            close_popover(&mut state, cx);
            check_update(&mut state, updates, true);
            refresh(&mut state, cx);
        }
        Action::InstallUpdate => {
            if state.demo || state.updating {
                return;
            }
            if let Some(release) = state.update.clone() {
                state.updating = true;
                state.model.update = UpdateState::Downloading;
                std::thread::spawn(move || {
                    let _ =
                        updates.send(UpdateEvent::Downloaded(updater::download(&release).is_ok()));
                });
            } else {
                check_update(&mut state, updates, true);
            }
            refresh(&mut state, cx);
        }
        Action::Quit => {
            state.config.position = native::position();
            save(&state);
            cx.quit();
        }
    }
}
fn save(state: &State) {
    if !state.demo && state.config.save().is_err() {
        tracing::warn!("cannot save overlay preferences");
    }
}
fn check_update(state: &mut State, events: Sender<UpdateEvent>, manual: bool) {
    if state.checking_update || state.updating || state.demo {
        return;
    }
    state.checking_update = true;
    state.next_update_check = Instant::now() + updater::CHECK_INTERVAL;
    if manual {
        state.model.update = UpdateState::Checking;
    }
    std::thread::spawn(move || {
        let _ = events.send(UpdateEvent::Checked(
            updater::check().map_err(|_| ()),
            manual,
        ));
    });
}
/// Routes a validated callback to the target it belongs to, creating it if needed.
fn accept_callback(state: &mut State, raw: &str) {
    let Some(callback) = parse_callback(raw) else {
        tracing::warn!("ignored an invalid statusbadge link");
        return;
    };
    let now = worker::now_ms();
    let Some(index) = state.pending.iter().position(|p| p.accepts(&callback, now)) else {
        tracing::warn!("ignored a statusbadge link without a matching request");
        return;
    };
    // One use only, even when the token is later refused.
    let pending = state.pending.remove(index);
    let existing = pending.target.clone().or_else(|| {
        state
            .entries
            .iter()
            .find(|e| e.target.base == pending.base)
            .map(|e| e.target.id.clone())
    });
    let id = existing.unwrap_or_else(|| {
        let mut random = [0u8; 16];
        rand::rng().fill_bytes(&mut random);
        random.iter().map(|b| format!("{b:02x}")).collect()
    });
    let target = Target {
        id: id.clone(),
        base: pending.base,
        name: callback.name,
        environment: callback.environment,
        summary: callback.summary,
        open: callback.open,
    };
    let session = worker::Session {
        token: callback.token,
        expires: callback.expires,
    };
    match state.entries.iter_mut().find(|e| e.target.id == id) {
        Some(entry) => {
            entry.state.message = Some("Vérification de la session…".into());
            match &entry.worker {
                Some(worker) => {
                    let _ = worker.send(worker::Command::Connect(target, session));
                }
                None => {
                    entry.worker = Some(worker::spawn(
                        target,
                        Some(session),
                        state.worker_events.clone(),
                    ))
                }
            }
        }
        None => {
            let worker = worker::spawn(target.clone(), Some(session), state.worker_events.clone());
            state.entries.push(Entry {
                target,
                state: TargetState::locked("Vérification de la session…"),
                worker: Some(worker),
                saved: false,
            });
            sort_entries(state);
        }
    }
}
fn sort_entries(state: &mut State) {
    let mut targets: Vec<Target> = state.entries.iter().map(|e| e.target.clone()).collect();
    sort_targets(&mut targets);
    state
        .entries
        .sort_by_key(|e| targets.iter().position(|t| t.id == e.target.id));
}
fn tick(
    shared: SharedState,
    events: Receiver<worker::Event>,
    urls: Receiver<String>,
    prompts: Receiver<String>,
    updates: Receiver<UpdateEvent>,
    update_tx: Sender<UpdateEvent>,
    cx: &mut App,
) {
    cx.spawn(async move |cx: &mut AsyncApp| loop {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        cx.update(|cx| {
            let mut state = shared.borrow_mut();
            let mut changed = false;
            for event in events.try_iter() {
                match event {
                    worker::Event::State(id, status) => {
                        if let Some(entry) = state.entries.iter_mut().find(|e| e.target.id == id) {
                            entry.state = status;
                            changed = true;
                        }
                    }
                    worker::Event::Connected(target) => {
                        let id = target.id.clone();
                        if let Some(entry) = state.entries.iter_mut().find(|e| e.target.id == id) {
                            entry.target = target.clone();
                            entry.saved = true;
                        }
                        state.config.targets.retain(|t| t.id != id);
                        state.config.targets.push(target);
                        sort_targets(&mut state.config.targets);
                        sort_entries(&mut state);
                        save(&state);
                        changed = true;
                    }
                    worker::Event::Rejected(id, message) => {
                        if let Some(index) = state.entries.iter().position(|e| e.target.id == id) {
                            if state.entries[index].saved {
                                state.entries[index].state.message = Some(message.into());
                            } else {
                                // Dropping its channel stops the worker; nothing was saved.
                                state.entries.remove(index);
                                state.prompting = false;
                                ask_for_sudo(&mut state, message);
                            }
                            changed = true;
                        }
                    }
                }
            }
            for raw in urls.try_iter() {
                if !state.demo {
                    accept_callback(&mut state, &raw);
                    changed = true;
                }
            }
            for answer in prompts.try_iter() {
                state.prompting = false;
                if answer.is_empty() {
                    continue;
                }
                match normalize_base(&answer) {
                    Some(base) => begin_login(&mut state, base, None, cx),
                    None => ask_for_sudo(
                        &mut state,
                        "URL refusée : HTTPS obligatoire (HTTP seulement pour localhost). Saisir l’adresse de l’interface sudo.",
                    ),
                }
            }
            for result in updates.try_iter() {
                match result {
                    UpdateEvent::Checked(result, manual) => {
                        state.checking_update = false;
                        match result {
                            Ok(Some(release)) => {
                                state.model.update =
                                    UpdateState::Available(release.version.to_string());
                                state.update = Some(release);
                            }
                            Ok(None) if manual => {
                                state.update = None;
                                state.model.update = UpdateState::Current;
                                state.update_notice_until = Some(Instant::now() + NOTICE);
                            }
                            Ok(None) => {}
                            Err(()) => {
                                state.next_update_check = Instant::now() + updater::RETRY_INTERVAL;
                                if manual {
                                    state.model.update = UpdateState::Failed;
                                    // With a known release, "!" stays: a click downloads it.
                                    if state.update.is_none() {
                                        state.update_notice_until = Some(Instant::now() + NOTICE);
                                    }
                                }
                            }
                        }
                    }
                    UpdateEvent::Downloaded(ok) => {
                        state.updating = false;
                        // The DMG is open in Finder. The badge keeps running until the
                        // installed bundle is replaced (see the bundle check below).
                        state.model.update = if ok {
                            UpdateState::Ready
                        } else {
                            UpdateState::Failed
                        };
                    }
                }
                changed = true;
            }
            state.model.now_ms = worker::now_ms();
            state.model.reduce_motion = native::reduce_motion();
            if state.demo {
                let now = worker::now_ms();
                for entry in &mut state.entries {
                    if let Some(summary) = &mut entry.state.summary {
                        summary.updated_at = Some(now);
                    }
                }
            }
            if state.update_notice_until.is_some_and(|at| Instant::now() >= at) {
                state.update_notice_until = None;
                if matches!(state.model.update, UpdateState::Current)
                    || matches!(state.model.update, UpdateState::Failed) && state.update.is_none()
                {
                    state.model.update = UpdateState::Idle;
                    changed = true;
                }
            }
            if state.bundle.is_some() && Instant::now() >= state.next_bundle_check {
                state.next_bundle_check = Instant::now() + Duration::from_secs(60);
                let replaced = state
                    .bundle
                    .as_deref()
                    .filter(|bundle| updater::bundle_was_updated(bundle));
                if let Some(bundle) = replaced {
                    updater::relaunch_after_exit(bundle);
                    state.config.position = native::position();
                    save(&state);
                    cx.quit();
                }
            }
            if Instant::now() >= state.next_update_check {
                check_update(&mut state, update_tx.clone(), false);
            }
            if native::pointer_over(native::BADGE)
                || state
                    .popover
                    .as_ref()
                    .is_some_and(|panel| native::pointer_over_window(&panel.native))
            {
                state.last_inside = Instant::now();
            } else if state.popover.as_ref().is_some_and(|p| p.mode != Mode::Menu)
                && state.last_inside.elapsed() > Duration::from_millis(180)
                || state.popover.as_ref().is_some_and(|p| p.mode == Mode::Menu)
                    && native::mouse_pressed()
            {
                close_popover(&mut state, cx);
            }
            let position = native::position();
            if position != state.last_position {
                state.last_position = position;
                state.position_changed = Some(Instant::now());
                close_popover(&mut state, cx);
            } else if state
                .position_changed
                .is_some_and(|at| at.elapsed() > Duration::from_millis(400))
            {
                state.position_changed = None;
                state.config.position = position;
                if state.demo {
                    tracing::info!(?position, "demo overlay position");
                }
                save(&state);
            }
            if state.drag.is_none() {
                native::keep_visible();
            }
            if changed {
                refresh(&mut state, cx);
            } else {
                // Only the badge redraws for the alarm; no network or I/O on animation ticks.
                state.badge.view.update(cx, |view, cx| {
                    view.model.now_ms = state.model.now_ms;
                    view.model.reduce_motion = state.model.reduce_motion;
                    if view
                        .model
                        .targets
                        .iter()
                        .any(|(_, s)| s.indicator(view.model.now_ms).0 == Status::Outage)
                    {
                        cx.notify();
                    }
                });
            }
        });
    })
    .detach();
}
