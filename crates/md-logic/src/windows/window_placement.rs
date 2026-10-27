//! Port of md.win `Windows/WindowPlacement.cs`.
//!
//! Where a new window goes and how big it is (shell-design.md §1.3, §9). Pure integer arithmetic
//! in GTK's logical pixels (md.win: WinUI's effective pixels).
//!
//! LINUX DECISION: GTK 4 can neither read nor set a toplevel's position — there is no
//! `gtk_window_move` on X11 or Wayland, and placement is the window manager's. The app uses the
//! sizes ([`parse_size`], [`format_size`], [`at_least_minimum`], `set_default_size`) and the
//! maximized flag; a position it records is `(0, 0)`. [`cascade`], [`centred`] and [`clamp`] are
//! kept whole, for parity and for the size they compute against a monitor's geometry
//! (`gdk::Monitor::geometry`, GTK 4 having no work-area call either).

use std::fmt;

/// A window's content size in logical pixels — what `md.linux.windowSize.*` holds, spelled
/// `"WxH"` (§9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

impl WindowSize {
    pub const fn new(width: i32, height: i32) -> Self {
        WindowSize { width, height }
    }
}

impl fmt::Display for WindowSize {
    /// The stored spelling: `"900x640"`, lower-case `x`, plain ASCII digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// A window's frame in logical pixels; `x` and `y` are the content area's top-left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl WindowRect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        WindowRect { x, y, width, height }
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn size(&self) -> WindowSize {
        WindowSize::new(self.width, self.height)
    }
}

/// The document window's default size (§1.3), and `md.linux.windowSize.document`'s default.
pub const DOCUMENT_DEFAULT: WindowSize = WindowSize::new(900, 640);

/// The Book window's default size (§1.3), and `md.linux.windowSize.book`'s default.
pub const BOOK_DEFAULT: WindowSize = WindowSize::new(1000, 700);

/// The windowed, non-Zen minimum (`gtk_widget_set_size_request` on the content, §1.3).
pub const MINIMUM: WindowSize = WindowSize::new(480, 320);

/// Each new window is offset this far right and down from the last active one (§1.3).
pub const CASCADE_STEP: i32 = 24;

/// Read a stored `"WxH"`. Anything that is not two positive integers around one `x` reads as
/// `fallback` — a corrupted setting must not make the app windowless. The result is never smaller
/// than [`MINIMUM`].
pub fn parse_size(stored: Option<&str>, fallback: WindowSize) -> WindowSize {
    let Some(stored) = stored else { return fallback };
    let Some(cut) = stored.find('x') else { return fallback };
    if cut == 0 || cut == stored.len() - 1 {
        return fallback;
    }
    let (Some(width), Some(height)) = (parse_digits(&stored[..cut]), parse_digits(&stored[cut + 1..])) else {
        return fallback;
    };
    if width <= 0 || height <= 0 {
        return fallback;
    }
    at_least_minimum(WindowSize::new(width, height))
}

/// .NET's `int.TryParse(…, NumberStyles.None, InvariantCulture)`: ASCII digits only — no sign, no
/// space, no separator — and within `i32`.
fn parse_digits(text: &str) -> Option<i32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The spelling [`parse_size`] reads back.
pub fn format_size(size: WindowSize) -> String {
    size.to_string()
}

/// Neither dimension below [`MINIMUM`].
pub fn at_least_minimum(size: WindowSize) -> WindowSize {
    WindowSize::new(size.width.max(MINIMUM.width), size.height.max(MINIMUM.height))
}

/// Where the next window goes: [`CASCADE_STEP`] right and down from the last active window, at
/// `size`. When that would push the window past the work area's right or bottom edge the cascade
/// restarts at the base position — the classic wrap, and what keeps the tenth window on screen.
/// The first window of a run (`previous` is `None`) is centred.
pub fn cascade(previous: Option<WindowRect>, size: WindowSize, work_area: WindowRect) -> WindowRect {
    let size = fit(at_least_minimum(size), work_area);
    let base = centred(size, work_area);
    let Some(last) = previous else { return clamp(base, work_area) };
    let mut next = WindowRect::new(last.x + CASCADE_STEP, last.y + CASCADE_STEP, size.width, size.height);
    if next.right() > work_area.right() || next.bottom() > work_area.bottom() {
        next = base;
    }
    clamp(next, work_area)
}

/// Centred in the work area — where the first window of a run lands.
pub fn centred(size: WindowSize, work_area: WindowRect) -> WindowRect {
    let size = fit(at_least_minimum(size), work_area);
    WindowRect::new(
        work_area.x + (work_area.width - size.width) / 2,
        work_area.y + (work_area.height - size.height) / 2,
        size.width,
        size.height,
    )
}

/// Bring a frame inside the work area: shrink it to fit first, then slide it in. This is what a
/// restored `session.json` frame goes through, so a window saved on a monitor that is no longer
/// attached — or one that was 4K and is now 1080p — still comes back reachable.
pub fn clamp(rect: WindowRect, work_area: WindowRect) -> WindowRect {
    let size = fit(WindowSize::new(rect.width, rect.height), work_area);
    let x = rect.x.clamp(work_area.x, work_area.x.max(work_area.right() - size.width));
    let y = rect.y.clamp(work_area.y, work_area.y.max(work_area.bottom() - size.height));
    WindowRect::new(x, y, size.width, size.height)
}

/// No bigger than the work area — a maximised-elsewhere frame must not exceed this screen.
fn fit(size: WindowSize, work_area: WindowRect) -> WindowSize {
    WindowSize::new(size.width.min(work_area.width.max(1)), size.height.min(work_area.height.max(1)))
}
