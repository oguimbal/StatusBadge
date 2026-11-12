use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{
    NSApplication, NSEvent, NSFloatingWindowLevel, NSScreen, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use statusbadge_ui::BADGE_SIZE;

pub const BADGE: &str = "Status Badge";
pub const POPOVER: &str = "Status Badge Details";
pub type NativeWindow = Retained<NSWindow>;

pub fn from_gpui(window: &gpui::Window) -> anyhow::Result<NativeWindow> {
    anyhow::ensure!(
        MainThreadMarker::new().is_some(),
        "AppKit requires the main thread"
    );
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| anyhow::anyhow!("native window handle unavailable"))?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        anyhow::bail!("expected an AppKit window");
    };
    // The borrowed GPUI handle guarantees a live NSView on this main thread.
    // Retain its exact window: recently closed panels can share the same title.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
        .ok_or_else(|| anyhow::anyhow!("view has no native window"))
}

pub fn accessory_application() {
    // GPUI resets LSUIElement applications to Regular when it finishes launching.
    // Restore the same accessory policy used by LeSwitcheur.
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
    }
}

fn native_window(title: &str) -> Option<Retained<NSWindow>> {
    let app = NSApplication::sharedApplication(MainThreadMarker::new()?);
    app.windows()
        .iter()
        .find(|window| window.title().to_string() == title)
}
pub fn configure(window: &NativeWindow, movable: bool) {
    window.setLevel(NSFloatingWindowLevel);
    window.setStyleMask(NSWindowStyleMask::NonactivatingPanel);
    window.setHasShadow(false);
    // NSPanel defaults to hiding on app deactivation, even at a floating level.
    // The badge must remain visible while another application has focus.
    window.setHidesOnDeactivate(false);
    window.setMovable(movable);
    window.setCollectionBehavior(
        window.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}
pub fn show(window: &NativeWindow, focus: bool) {
    if focus {
        window.makeKeyAndOrderFront(None);
    } else {
        window.orderFrontRegardless();
    }
}
pub fn hide(window: &NativeWindow) {
    window.orderOut(None);
}
fn screens() -> Vec<NSRect> {
    MainThreadMarker::new()
        .map(|mtm| {
            NSScreen::screens(mtm)
                .iter()
                .map(|s| s.visibleFrame())
                .collect()
        })
        .unwrap_or_default()
}
fn contains(frame: NSRect, p: NSPoint) -> bool {
    p.x >= frame.origin.x
        && p.y >= frame.origin.y
        && p.x <= frame.origin.x + frame.size.width
        && p.y <= frame.origin.y + frame.size.height
}
pub fn position() -> Option<[f64; 2]> {
    native_window(BADGE).map(|w| [w.frame().origin.x, w.frame().origin.y])
}
/// The badge may hang over a screen edge, as long as its centre stays this far inside.
const CENTER_MARGIN: f64 = 14.0;

fn inset(frame: NSRect) -> NSRect {
    NSRect::new(
        NSPoint::new(
            frame.origin.x + CENTER_MARGIN,
            frame.origin.y + CENTER_MARGIN,
        ),
        NSSize::new(
            (frame.size.width - 2.0 * CENTER_MARGIN).max(0.0),
            (frame.size.height - 2.0 * CENTER_MARGIN).max(0.0),
        ),
    )
}
fn clamp_into(frame: NSRect, p: NSPoint) -> NSPoint {
    NSPoint::new(
        p.x.clamp(frame.origin.x, frame.origin.x + frame.size.width),
        p.y.clamp(frame.origin.y, frame.origin.y + frame.size.height),
    )
}
/// Origin keeping the badge centre on the nearest screen, so the window always overlaps
/// one: GPUI aborts when a window ends up on no screen at all.
fn clamped_origin(origin: NSPoint) -> NSPoint {
    let half = f64::from(BADGE_SIZE) / 2.0;
    let center = NSPoint::new(origin.x + half, origin.y + half);
    screens()
        .into_iter()
        .map(|screen| clamp_into(inset(screen), center))
        .min_by(|a, b| {
            let d = |p: &NSPoint| (p.x - center.x).hypot(p.y - center.y);
            d(a).total_cmp(&d(b))
        })
        .map_or(origin, |c| NSPoint::new(c.x - half, c.y - half))
}
/// Moves the badge to `saved` (or the default bottom-left corner), pulling its centre back
/// onto the nearest screen when it went too far: never a jump to another corner.
pub fn place_badge(saved: Option<[f64; 2]>) {
    let size = f64::from(BADGE_SIZE);
    let Some(window) = native_window(BADGE) else {
        return;
    };
    let Some(primary) = screens().first().copied() else {
        return;
    };
    let origin = saved
        .filter(|p| p.iter().all(|n| n.is_finite()))
        .map(|p| NSPoint::new(p[0], p[1]))
        .unwrap_or(NSPoint::new(
            primary.origin.x + 24.0,
            primary.origin.y + 24.0,
        ));
    window.setFrame_display(
        NSRect::new(clamped_origin(origin), NSSize::new(size, size)),
        true,
    );
}
pub fn anchor_popover(popover: &NativeWindow, width: f64, height: f64) {
    let Some(badge) = native_window(BADGE) else {
        return;
    };
    let frame = badge.frame();
    let Some(screen) = badge.screen().map(|s| s.visibleFrame()) else {
        return;
    };
    let x = frame.origin.x.clamp(
        screen.origin.x + 4.0,
        (screen.origin.x + screen.size.width - width - 4.0).max(screen.origin.x + 4.0),
    );
    let above = frame.origin.y + frame.size.height + 6.0;
    let y = if above + height <= screen.origin.y + screen.size.height {
        above
    } else {
        frame.origin.y - height - 6.0
    };
    popover.setFrame_display(
        NSRect::new(
            NSPoint::new(x, y.max(screen.origin.y + 4.0)),
            NSSize::new(width, height),
        ),
        true,
    );
    tracing::debug!(
        window = popover.windowNumber(),
        badge = ?frame,
        popover = ?popover.frame(),
        "popover anchored before showing"
    );
}
pub fn pointer_over(title: &str) -> bool {
    native_window(title).is_some_and(|w| pointer_over_window(&w))
}
pub fn pointer_over_window(window: &NativeWindow) -> bool {
    window.isVisible() && contains(window.frame(), NSEvent::mouseLocation())
}
pub fn mouse_pressed() -> bool {
    NSEvent::pressedMouseButtons() != 0
}
pub fn move_badge(position: [f64; 2]) {
    if let Some(window) = native_window(BADGE) {
        window.setFrameOrigin(clamped_origin(NSPoint::new(position[0], position[1])));
    }
}
pub fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

pub fn keep_visible() {
    let Some(window) = native_window(BADGE) else {
        return;
    };
    let frame = window.frame();
    let center = NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    if !screens()
        .into_iter()
        .any(|screen| contains(inset(screen), center))
    {
        place_badge(Some([frame.origin.x, frame.origin.y]));
    }
}
