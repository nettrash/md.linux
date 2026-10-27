//! `md --selftest OUT [--only g,…] [--no-input]` — the in-app self-test (shell-design §13), compiled
//! only with the cargo feature `selftest`.
//!
//! The shell cannot be unit-tested without a display, so its proofs run inside the real app: a
//! private (`NON_UNIQUE`) process whose profile lives under `OUT/home/` (main.rs refuses anything
//! else) opens its windows as usual, then this runner drives them group by group, records named
//! checks, and writes `OUT/report.json`:
//!
//! ```json
//! {"started": "…", "finished": "…", "groups": ["shell", …],
//!  "checks": [{"name": "shell.app_starts", "passed": true, "detail": "…"}]}
//! ```
//!
//! The process exits 0 when every check passed and the run finished inside its budget, else 1.
//! Screenshots go to `OUT/screenshots/` (ImageMagick `import` of the Xvfb root window), real
//! keystrokes and clicks come from `xdotool` (XTEST); both are in the dev image.
//!
//! Each group lives in its own file, owned by the work package it proves
//! (docs/shell-work-packages.md §2): `async fn run(h: &Harness)`, checks through [`Harness::check`].
//! `--no-input` skips the keystroke tier: groups ask [`Harness::input`] before typing.

pub mod books;
pub mod documents;
pub mod editor;
pub mod exports;
pub mod preview;
pub mod privacy;
pub mod shell;

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use md_logic::exec::Completion;

use crate::app::Shell;

/// The variables that must point into `OUT/` (checked in main.rs before anything starts).
pub const XDG_VARIABLES: [&str; 4] = ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"];

/// Every group, in run order; `privacy` looks at what all the others left behind.
pub const GROUPS: [&str; 7] = ["shell", "documents", "editor", "preview", "exports", "books", "privacy"];

/// The report's file name in `OUT`.
pub const REPORT_FILE_NAME: &str = "report.json";

/// The whole run's ceiling; a run that hits it fails with a `selftest.completed` row.
const BUDGET: Duration = Duration::from_secs(20 * 60);

/// How long the first window may take to appear.
const FIRST_WINDOW_LIMIT: Duration = Duration::from_secs(30);

const FLAG: &str = "--selftest";
const ONLY: &str = "--only";
const NO_INPUT: &str = "--no-input";

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The output folder: report, screenshots, and the redirected profile under `home/`.
    pub out: PathBuf,
    /// The groups to run, in [`GROUPS`] order.
    pub groups: Vec<&'static str>,
    /// False with `--no-input`: no `xdotool` keystrokes or clicks.
    pub input: bool,
}

impl Options {
    /// Takes `--selftest OUT [--only g,…] [--no-input]` out of `args`, leaving the rest for
    /// GApplication. `Ok(None)` when there is no `--selftest`; `--only` and `--no-input` mean
    /// nothing without it and are left alone (GLib then rejects them as unknown options).
    pub fn take(args: &mut Vec<OsString>) -> Result<Option<Options>, String> {
        let Some(at) = args.iter().position(|a| a == FLAG) else { return Ok(None) };
        let usage = || format!("usage: md {FLAG} OUT [{ONLY} group,…] [{NO_INPUT}]; groups: {}", GROUPS.join(", "));
        let out = match args.get(at + 1) {
            Some(out) if !out.to_string_lossy().starts_with('-') && !out.is_empty() => PathBuf::from(out),
            _ => return Err(usage()),
        };
        args.drain(at..at + 2);

        let mut input = true;
        if let Some(at) = args.iter().position(|a| a == NO_INPUT) {
            args.remove(at);
            input = false;
        }

        let mut groups: Vec<&'static str> = GROUPS.to_vec();
        let only_at = args.iter().position(|a| a == ONLY || a.to_string_lossy().starts_with("--only="));
        if let Some(at) = only_at {
            let arg = args.remove(at).to_string_lossy().into_owned();
            let list = match arg.strip_prefix("--only=") {
                Some(list) => list.to_string(),
                None if at < args.len() => args.remove(at).to_string_lossy().into_owned(),
                None => return Err(usage()),
            };
            let mut wanted = Vec::new();
            for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match GROUPS.iter().find(|g| **g == name) {
                    Some(group) => wanted.push(*group),
                    None => return Err(format!("unknown self-test group \"{name}\"; {}", usage())),
                }
            }
            if wanted.is_empty() {
                return Err(usage());
            }
            groups.retain(|g| wanted.contains(g));
        }

        Ok(Some(Options { out, groups, input }))
    }
}

/// One row of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Check {
    name: String,
    passed: bool,
    detail: String,
}

/// What every group drives the app with: checks, waits, screenshots, input, child processes.
pub struct Harness {
    out: PathBuf,
    input: bool,
    group: Cell<&'static str>,
    checks: RefCell<Vec<Check>>,
}

impl Harness {
    fn new(options: &Options) -> Harness {
        Harness {
            out: options.out.clone(),
            input: options.input,
            group: Cell::new(""),
            checks: RefCell::new(Vec::new()),
        }
    }

    fn record(&self, name: String, passed: bool, detail: String) -> bool {
        println!(
            "CHECK {} {name}{}",
            if passed { "pass" } else { "FAIL" },
            if detail.is_empty() { String::new() } else { format!(" — {detail}") }
        );
        self.checks.borrow_mut().push(Check { name, passed, detail });
        passed
    }

    fn all_passed(&self) -> bool {
        self.checks.borrow().iter().all(|c| c.passed)
    }

    /// The group being run.
    pub fn group(&self) -> &'static str {
        self.group.get()
    }

    /// Records `<group>.<name>`; returns `passed` so a group can stop early.
    pub fn check(&self, name: &str, passed: bool, detail: impl Into<String>) -> bool {
        let name = match self.group() {
            "" => name.to_string(),
            group => format!("{group}.{name}"),
        };
        self.record(name, passed, detail.into())
    }

    pub async fn sleep(&self, duration: Duration) {
        glib::timeout_future(duration).await;
    }

    /// Polls `condition` every 50 ms; true as soon as it holds, false after `limit`.
    pub async fn wait_until(&self, limit: Duration, condition: impl Fn() -> bool) -> bool {
        let start = std::time::Instant::now();
        loop {
            if condition() {
                return true;
            }
            if start.elapsed() >= limit {
                return false;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
    }

    /// `OUT/screenshots/<group>-<name>.png` of the whole Xvfb screen, after one more frame's
    /// worth of time. The path, or `None` (and the reason on stderr).
    pub async fn screenshot(&self, name: &str) -> Option<PathBuf> {
        self.sleep(Duration::from_millis(300)).await;
        let folder = self.out.join("screenshots");
        if let Err(err) = std::fs::create_dir_all(&folder) {
            eprintln!("md selftest: cannot create {}: {err}", folder.display());
            return None;
        }
        let file = match self.group() {
            "" => format!("{name}.png"),
            group => format!("{group}-{name}.png"),
        };
        let path = folder.join(file);
        let (ok, output) = self.run(&["import", "-window", "root", &path.to_string_lossy()]).await;
        if ok {
            println!("SHOT {}", path.display());
            Some(path)
        } else {
            eprintln!("md selftest: screenshot {name} failed: {output}");
            None
        }
    }

    /// Runs a program to completion without blocking the main loop: (success, stdout + stderr).
    pub async fn run(&self, argv: &[&str]) -> (bool, String) {
        let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
        let flags = gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE;
        let process = match gio::Subprocess::newv(&argv, flags) {
            Ok(process) => process,
            Err(err) => return (false, err.to_string()),
        };
        match process.communicate_utf8_future(None).await {
            Ok((out, _)) => (process.is_successful(), out.map(|s| s.to_string()).unwrap_or_default()),
            Err(err) => (false, err.to_string()),
        }
    }

    fn report_json(&self, started: &str, finished: &str, groups: &[&str]) -> String {
        let mut out = String::from("{\n");
        let _ = writeln!(out, "  \"started\": {},", json_string(started));
        let _ = writeln!(out, "  \"finished\": {},", json_string(finished));
        let groups: Vec<String> = groups.iter().map(|g| json_string(g)).collect();
        let _ = writeln!(out, "  \"groups\": [{}],", groups.join(", "));
        let checks: Vec<String> = self
            .checks
            .borrow()
            .iter()
            .map(|c| {
                format!(
                    "    {{\"name\": {}, \"passed\": {}, \"detail\": {}}}",
                    json_string(&c.name),
                    c.passed,
                    json_string(&c.detail)
                )
            })
            .collect();
        if checks.is_empty() {
            out.push_str("  \"checks\": []\n");
        } else {
            let _ = writeln!(out, "  \"checks\": [\n{}\n  ]", checks.join(",\n"));
        }
        out.push_str("}\n");
        out
    }
}

/// The group-facing half of the harness: what the groups (P1–P5) drive the app with.
impl Harness {
    /// The process's shell.
    pub fn shell(&self) -> Rc<Shell> {
        Shell::get()
    }

    /// The output folder (`OUT`).
    pub fn out(&self) -> &Path {
        &self.out
    }

    /// The redirected profile, `OUT/home`.
    pub fn home(&self) -> PathBuf {
        self.out.join("home")
    }

    /// False with `--no-input`: skip the keystroke rows.
    pub fn input(&self) -> bool {
        self.input
    }

    /// `future`, or `None` after `limit`.
    pub async fn with_timeout<T>(&self, limit: Duration, future: impl std::future::Future<Output = T>) -> Option<T> {
        glib::future_with_timeout(limit, future).await.ok()
    }

    /// The next frame `widget` draws (at most 2 s: an unmapped widget never ticks).
    pub async fn next_frame(&self, widget: &impl IsA<gtk::Widget>) {
        let done: Completion<()> = Completion::new();
        let ticked = done.clone();
        widget.add_tick_callback(move |_, _| {
            ticked.complete(());
            glib::ControlFlow::Break
        });
        let _ = glib::future_with_timeout(Duration::from_secs(2), done.wait()).await;
    }

    /// A second `md` for relaunch checks, with this run's XDG variables passed on explicitly (the
    /// process cannot set them for itself; see main.rs). The caller adds `--selftest`-free
    /// arguments; the child shares `OUT/home`.
    pub fn md_command(&self) -> std::process::Command {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("md"));
        let mut command = std::process::Command::new(exe);
        command.envs(XDG_VARIABLES.iter().filter_map(|v| std::env::var_os(v).map(|value| (*v, value))));
        command
    }

    // Input: XTEST through xdotool; Xvfb with no window manager gives md the focus.

    pub async fn xdotool(&self, args: &[&str]) -> (bool, String) {
        let mut argv = vec!["xdotool"];
        argv.extend_from_slice(args);
        self.run(&argv).await
    }

    /// Presses a chord in xdotool's spelling (`ctrl+z`, `shift+Return`, `ctrl+alt+2`). False with
    /// `--no-input`.
    pub async fn key(&self, chord: &str) -> bool {
        if !self.input {
            return false;
        }
        let (ok, _) = self.xdotool(&["key", "--clearmodifiers", chord]).await;
        self.sleep(Duration::from_millis(80)).await;
        ok
    }

    /// Types `text` key by key. False with `--no-input`.
    pub async fn type_text(&self, text: &str) -> bool {
        if !self.input {
            return false;
        }
        let (ok, _) = self.xdotool(&["type", "--delay", "40", "--", text]).await;
        self.sleep(Duration::from_millis(80)).await;
        ok
    }

    /// The X11 window id of `window`, found by its title (the last match when several share it).
    pub async fn xid(&self, window: &gtk::Window) -> Option<String> {
        let title = window.title().map(|t| t.to_string()).unwrap_or_default();
        let pattern = format!("^{}$", Self::regex_escape(&title));
        let (_, ids) = self.xdotool(&["search", "--onlyvisible", "--name", &pattern]).await;
        ids.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string)
    }

    /// Gives `window` the keyboard focus. False with `--no-input` or when it cannot be found.
    pub async fn focus(&self, window: &gtk::Window) -> bool {
        if !self.input {
            return false;
        }
        let Some(xid) = self.xid(window).await else { return false };
        let (ok, _) = self.xdotool(&["windowfocus", "--sync", &xid]).await;
        self.sleep(Duration::from_millis(150)).await;
        ok
    }

    /// Clicks button 1 at (`x`, `y`) in `widget`'s coordinates. False with `--no-input`.
    pub async fn click(&self, window: &gtk::Window, widget: &impl IsA<gtk::Widget>, x: f64, y: f64) -> bool {
        if !self.input {
            return false;
        }
        let Some((sx, sy)) = self.screen_point(window, widget, x, y).await else { return false };
        let (ok, _) = self.xdotool(&["mousemove", "--sync", &sx.to_string(), &sy.to_string(), "click", "1"]).await;
        self.sleep(Duration::from_millis(300)).await;
        ok
    }

    /// Screen coordinates of a point in `widget`: the X11 window's origin plus the surface
    /// transform plus the widget's offset in the window. xdotool and the X server count device
    /// pixels, GTK lays out in logical ones: under `GDK_SCALE=2` everything inside the window is
    /// twice as far from its origin on screen.
    async fn screen_point(
        &self,
        window: &gtk::Window,
        widget: &impl IsA<gtk::Widget>,
        x: f64,
        y: f64,
    ) -> Option<(i32, i32)> {
        let xid = self.xid(window).await?;
        let (_, geometry) = self.xdotool(&["getwindowgeometry", "--shell", &xid]).await;
        let field =
            |name: &str| geometry.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.parse::<f64>().ok());
        let (wx, wy) = (field("X=")?, field("Y=")?);
        let point = widget.compute_point(window, &gtk::graphene::Point::new(x as f32, y as f32))?;
        let (sx, sy) = window.surface_transform();
        let scale = window.surface().map_or(1.0, |surface| surface.scale());
        let device = |origin: f64, offset: f64| (origin + (offset * scale)).round() as i32;
        Some((device(wx, sx + f64::from(point.x())), device(wy, sy + f64::from(point.y()))))
    }

    /// Every toplevel GTK has now — to tell a dialog that appears later from what was there.
    pub fn toplevels() -> Vec<gtk::Window> {
        gtk::Window::list_toplevels().into_iter().filter_map(|w| w.downcast().ok()).collect()
    }

    /// Waits for GTK's print dialog (a new visible toplevel whose type names it), screenshots it as
    /// `shot` and cancels it — Escape through xdotool when input is on, else `close()`. Returns the
    /// dialog's type name and how it went, or `None` when no dialog appeared.
    pub async fn dismiss_print_dialog(&self, known: Vec<gtk::Window>, shot: &str) -> Option<(String, &'static str)> {
        let dialog = Rc::new(RefCell::new(None::<gtk::Window>));
        let found = {
            let dialog = Rc::clone(&dialog);
            self.wait_until(Duration::from_secs(60), move || {
                let fresh = gtk::Window::list_toplevels()
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk::Window>().ok())
                    .find(|w| !known.contains(w) && w.is_visible() && w.type_().name().contains("Print"));
                let found = fresh.is_some();
                *dialog.borrow_mut() = fresh;
                found
            })
            .await
        };
        let window = dialog.borrow().clone();
        let window = window.filter(|_| found)?;
        let kind = window.type_().name().to_string();
        self.screenshot(shot).await;
        let mut how = "Escape";
        if !(self.focus(&window).await && self.key("Escape").await) {
            how = "close()";
            window.close();
        }
        if !self.wait_until(Duration::from_secs(5), || !window.is_visible()).await {
            how = "close() after Escape";
            window.close();
        }
        Some((kind, how))
    }

    /// `s` as a POSIX extended regular expression matching itself (xdotool's `--name`).
    fn regex_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if "\\^$.|?*+()[]{}".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }
}

/// A JSON string literal (RFC 8259: quote, backslash and every control character escaped).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn now_iso8601() -> String {
    glib::DateTime::now_utc().and_then(|t| t.format_iso8601()).map(|s| s.to_string()).unwrap_or_default()
}

thread_local! {
    /// The run's verdict, once the report is written.
    static OUTCOME: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Called in `startup`: holds the application and spawns the runner, which waits for the first
/// window, runs the groups, writes the report and quits.
pub fn start(shell: &Rc<Shell>, options: Options) {
    let hold = shell.app.hold();
    let app = shell.app.clone();
    glib::spawn_future_local(async move {
        if let Err(err) = std::fs::create_dir_all(&options.out) {
            eprintln!("md selftest: cannot create {}: {err}", options.out.display());
        }
        let harness = Rc::new(Harness::new(&options));
        let started = now_iso8601();
        println!("selftest: {} → {}", options.groups.join(", "), options.out.display());
        let completed =
            glib::future_with_timeout(BUDGET, run_groups(Rc::clone(&harness), options.groups.clone())).await.is_ok();
        if !completed {
            harness.record(
                "selftest.completed".to_string(),
                false,
                format!("the run exceeded its {} min budget in group \"{}\"", BUDGET.as_secs() / 60, harness.group()),
            );
        }
        let finished = now_iso8601();
        let report = options.out.join(REPORT_FILE_NAME);
        let written = std::fs::write(&report, harness.report_json(&started, &finished, &options.groups));
        let passed = match written {
            Ok(()) => {
                let checks = harness.checks.borrow();
                let failed = checks.iter().filter(|c| !c.passed).count();
                println!("selftest: {} checks, {failed} failed → {}", checks.len(), report.display());
                completed && harness.all_passed()
            }
            Err(err) => {
                eprintln!("md selftest: cannot write {}: {err}", report.display());
                false
            }
        };
        OUTCOME.with(|o| o.set(Some(passed)));
        drop(hold);
        app.quit();
    });
}

async fn run_groups(harness: Rc<Harness>, groups: Vec<&'static str>) {
    let app = Shell::get().app.clone();
    if !harness.wait_until(FIRST_WINDOW_LIMIT, || !app.windows().is_empty()).await {
        eprintln!("md selftest: no window appeared within {} s", FIRST_WINDOW_LIMIT.as_secs());
    }
    harness.screenshot("start").await;
    for group in groups {
        harness.group.set(group);
        println!("selftest: group {group}");
        let h: &Harness = &harness;
        match group {
            "shell" => shell::run(h).await,
            "documents" => documents::run(h).await,
            "editor" => editor::run(h).await,
            "preview" => preview::run(h).await,
            "exports" => exports::run(h).await,
            "books" => books::run(h).await,
            "privacy" => privacy::run(h).await,
            other => {
                harness.check("unknown", false, format!("no such group: {other}"));
            }
        }
    }
}

/// The process's exit code: success only when the report was written and every check passed.
pub fn exit_code() -> glib::ExitCode {
    match OUTCOME.with(Cell::get) {
        Some(true) => glib::ExitCode::SUCCESS,
        _ => glib::ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn without_the_flag_nothing_is_taken() {
        let mut a = args(&["md", "--only", "shell", "notes.md"]);
        assert_eq!(Options::take(&mut a), Ok(None));
        assert_eq!(a, args(&["md", "--only", "shell", "notes.md"]));
    }

    #[test]
    fn the_flags_are_taken_and_the_files_left() {
        let mut a = args(&["md", "--selftest", "/out/selftest", "--only", "privacy,shell", "--no-input", "a.md"]);
        let options = Options::take(&mut a).unwrap().unwrap();
        assert_eq!(options.out, PathBuf::from("/out/selftest"));
        assert_eq!(options.groups, vec!["shell", "privacy"]);
        assert!(!options.input);
        assert_eq!(a, args(&["md", "a.md"]));

        let mut a = args(&["md", "--only=editor", "--selftest", "out"]);
        let options = Options::take(&mut a).unwrap().unwrap();
        assert_eq!(options.groups, vec!["editor"]);
        assert!(options.input);
        assert_eq!(a, args(&["md"]));

        let mut a = args(&["md", "--selftest", "out"]);
        assert_eq!(Options::take(&mut a).unwrap().unwrap().groups, GROUPS.to_vec());
    }

    #[test]
    fn bad_command_lines_are_refused() {
        for bad in [
            &["md", "--selftest"][..],
            &["md", "--selftest", "--no-input"],
            &["md", "--selftest", "out", "--only"],
            &["md", "--selftest", "out", "--only", "shell,nope"],
            &["md", "--selftest", "out", "--only="],
        ] {
            assert!(Options::take(&mut args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn json_strings_escape_what_rfc_8259_requires() {
        assert_eq!(json_string("a\"b\\c\nd\u{1}\u{1f}é — \u{1F600}"), "\"a\\\"b\\\\c\\nd\\u0001\\u001fé — \u{1F600}\"");
    }

    #[test]
    fn an_empty_run_reports_zero_checks() {
        let options = Options { out: PathBuf::from("/out"), groups: vec!["shell"], input: true };
        let harness = Harness::new(&options);
        let json = harness.report_json("2026-10-27T10:00:00Z", "2026-10-27T10:00:01Z", &options.groups);
        assert_eq!(
            json,
            "{\n  \"started\": \"2026-10-27T10:00:00Z\",\n  \"finished\": \"2026-10-27T10:00:01Z\",\n  \"groups\": [\"shell\"],\n  \"checks\": []\n}\n"
        );
        assert!(harness.all_passed());
        harness.group.set("shell");
        harness.check("one", true, "");
        harness.check("two", false, "why \"not\"");
        let json = harness.report_json("s", "f", &options.groups);
        assert!(json.contains("{\"name\": \"shell.one\", \"passed\": true, \"detail\": \"\"},\n"));
        assert!(json.contains("{\"name\": \"shell.two\", \"passed\": false, \"detail\": \"why \\\"not\\\"\"}\n  ]"));
        assert!(!harness.all_passed());
    }

    #[test]
    fn titles_become_literal_patterns() {
        assert_eq!(Harness::regex_escape("Untitled 2 — Edited"), "Untitled 2 — Edited");
        assert_eq!(Harness::regex_escape("a.b (c) [d] $1 x*y+z?"), "a\\.b \\(c\\) \\[d\\] \\$1 x\\*y\\+z\\?");
    }
}
