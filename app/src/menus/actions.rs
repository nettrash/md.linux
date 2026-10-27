//! One `gio::SimpleAction` per `CommandId` (shell-design §2.2): kebab-case names in the `app.` or
//! `win.` scope, radio groups as stateful actions, parametrised rows, enablement and ticks from the
//! window's `ShellSnapshot`, the editing chords dispatched to the focused widget, and every
//! accelerator installed once in `startup`.
//!
//! The mapping is pure ([`shape`], [`detailed_name`], unit-tested: every command one unique action
//! and scope). The GTK half keeps, per window, the actions [`install_window_actions`] created, so
//! [`refresh`] (called by `MenuBar::refresh`) can set `enabled` and
//! `state` from a snapshot.
//!
//! Activation never changes an action's state by itself: every `activate` handler is connected, so
//! GTK's default (toggle a boolean, adopt a radio target) never runs, and a tick follows only the
//! snapshot — re-clicking a ticked row keeps the tick.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use md_core::document::view_mode::ViewMode;
use md_logic::commands::{
    ActionScope, CommandArgument, CommandDispatcher, CommandId, CommandKind, ShellSnapshot, command_enablement,
    command_table,
};
use md_logic::seams::WindowId;

use crate::app::Shell;
use crate::menus::accels;
use crate::windows::host::WindowKind;

/// An action's parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Parameter {
    None,
    /// `s`: an Open Recent token, an example's file name, a page size id, a view mode.
    Text,
    /// `u`: a Contents / Notes row's index in the snapshot, a diagram's ordinal.
    Index,
    /// `t`: a window id (the Window list).
    Window,
}

/// An action's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    None,
    /// `b`: a check row.
    Bool,
    /// `s`: a radio group (View mode, PDF Page Size).
    Text,
    /// `t`: the Window list — each window's state is its own id, so its menu ticks itself.
    Window,
}

/// How one command becomes an action.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActionShape {
    /// md-logic's [`CommandId::scope`]: `app.` or `win.`.
    pub scope: ActionScope,
    /// The action's name, without the scope.
    pub name: String,
    /// The fixed target a row carries (the View-mode radios); dynamic rows get theirs per row.
    pub target: Option<&'static str>,
    pub parameter: Parameter,
    pub state: State,
}

/// The one radio group shared by three commands.
const VIEW_MODE_ACTION: &str = "view-mode";

/// The View radio's target for a mode (its raw value: `edit` / `split` / `preview`).
fn view_target(id: CommandId) -> Option<&'static str> {
    match id {
        CommandId::ViewEdit => Some(ViewMode::Edit.raw_value()),
        CommandId::ViewSplit => Some(ViewMode::Split.raw_value()),
        CommandId::ViewPreview => Some(ViewMode::Preview.raw_value()),
        _ => None,
    }
}

/// The command a View radio target stands for.
fn view_command(target: &str) -> Option<CommandId> {
    [CommandId::ViewEdit, CommandId::ViewSplit, CommandId::ViewPreview]
        .into_iter()
        .find(|id| view_target(*id) == Some(target))
}

/// The action `id` is, or `None` for Escape — [`CommandKind::WindowKey`], the window's own key
/// controller on Linux (§2.2).
pub fn shape(id: CommandId) -> Option<ActionShape> {
    let spec = command_table::spec(id);
    if spec.kind == CommandKind::WindowKey {
        return None;
    }
    let scope = id.scope();
    if let Some(target) = view_target(id) {
        return Some(ActionShape {
            scope,
            name: VIEW_MODE_ACTION.to_string(),
            target: Some(target),
            parameter: Parameter::Text,
            state: State::Text,
        });
    }
    let (parameter, state) = match (id, spec.kind) {
        (CommandId::ActivateWindow, _) => (Parameter::Window, State::Window),
        (CommandId::PdfPageSize, _) => (Parameter::Text, State::Text),
        (CommandId::OpenRecentEntry | CommandId::Example, _) => (Parameter::Text, State::None),
        (CommandId::Contents | CommandId::Notes | CommandId::ExportDiagramSvg, _) => (Parameter::Index, State::None),
        (_, CommandKind::Toggle) => (Parameter::None, State::Bool),
        _ => (Parameter::None, State::None),
    };
    Some(ActionShape { scope, name: id.action_name(), target: None, parameter, state })
}

/// `app.new`, `win.save-as`, `win.view-mode::edit` — what a static row and its accelerator name.
/// Dynamic rows carry their own targets.
pub fn detailed_name(id: CommandId) -> Option<String> {
    let shape = shape(id)?;
    Some(match shape.target {
        Some(target) => format!("{}.{}::{target}", shape.scope.prefix(), shape.name),
        None => format!("{}.{}", shape.scope.prefix(), shape.name),
    })
}

/// The `win.` action a greyed submenu row stands on: always disabled (see `menu_bar`).
pub const UNAVAILABLE: &str = "unavailable";

/// The argument an activation carries, from the action's parameter and (for the index rows) the
/// window's current snapshot. `None` when the row no longer exists.
fn argument(
    id: CommandId,
    parameter: Option<&glib::Variant>,
    snapshot: impl FnOnce() -> ShellSnapshot,
) -> Option<(CommandId, CommandArgument)> {
    if id == CommandId::ViewEdit {
        let target = parameter.and_then(|p| p.str())?;
        return Some((view_command(target)?, CommandArgument::None));
    }
    let shape = shape(id)?;
    let argument = match shape.parameter {
        Parameter::None => CommandArgument::None,
        Parameter::Text => CommandArgument::Text(parameter.and_then(|p| p.str())?.to_string()),
        Parameter::Window => CommandArgument::Window(WindowId(parameter.and_then(|p| p.get::<u64>())?)),
        Parameter::Index => {
            let index = parameter.and_then(|p| p.get::<u32>())? as usize;
            match id {
                CommandId::Contents => CommandArgument::Outline(snapshot().outline.get(index)?.clone()),
                CommandId::Notes => CommandArgument::Note(snapshot().notes.get(index)?.clone()),
                _ => CommandArgument::Diagram(index),
            }
        }
    };
    Some((id, argument))
}

/// A fresh action of `shape`, stateless or in its neutral state.
fn new_action(shape: &ActionShape) -> gio::SimpleAction {
    let parameter = match shape.parameter {
        Parameter::None => None,
        Parameter::Text => Some(glib::VariantTy::STRING),
        Parameter::Index => Some(glib::VariantTy::UINT32),
        Parameter::Window => Some(glib::VariantTy::UINT64),
    };
    match shape.state {
        State::None => gio::SimpleAction::new(&shape.name, parameter),
        State::Bool => gio::SimpleAction::new_stateful(&shape.name, parameter, &false.to_variant()),
        State::Text => gio::SimpleAction::new_stateful(&shape.name, parameter, &"".to_variant()),
        State::Window => gio::SimpleAction::new_stateful(&shape.name, parameter, &0u64.to_variant()),
    }
}

/// The ids whose actions live in `scope`, one per action (the View radios share theirs; ViewEdit
/// stands for the group).
fn commands_in(scope: ActionScope) -> impl Iterator<Item = CommandId> {
    CommandId::ALL.into_iter().filter(move |id| {
        !matches!(id, CommandId::ViewSplit | CommandId::ViewPreview)
            && shape(*id).is_some_and(|shape| shape.scope == scope)
    })
}

/// Whether the action standing for `id` is enabled: the View radio is live when any of its three
/// rows is.
fn action_enabled(id: CommandId, snapshot: &ShellSnapshot) -> bool {
    if id == CommandId::ViewEdit {
        return [CommandId::ViewEdit, CommandId::ViewSplit, CommandId::ViewPreview]
            .into_iter()
            .any(|id| command_enablement::is_enabled(id, snapshot));
    }
    command_enablement::is_enabled(id, snapshot)
}

/// The state the action standing for `id` shows for `snapshot`, or `None` for a stateless one.
fn action_state(id: CommandId, snapshot: &ShellSnapshot) -> Option<glib::Variant> {
    match shape(id)?.state {
        State::None => None,
        State::Bool => Some(command_enablement::is_checked(id, snapshot).to_variant()),
        State::Text if id == CommandId::ViewEdit => Some(snapshot.published_mode().raw_value().to_variant()),
        State::Text => Some(snapshot.pdf_page_size_id.to_variant()),
        State::Window => Some(snapshot.window_titles.iter().find(|w| w.is_this).map_or(0u64, |w| w.id.0).to_variant()),
    }
}

/// Sets `action`'s enabled flag and state, touching neither when unchanged (no notify storm).
fn apply_one(action: &gio::SimpleAction, enabled: bool, state: Option<glib::Variant>) {
    if action.is_enabled() != enabled {
        action.set_enabled(enabled);
    }
    if let Some(state) = state
        && action.state().as_ref() != Some(&state)
    {
        action.set_state(&state);
    }
}

/// One window's actions.
struct WindowActions {
    window: glib::WeakRef<adw::ApplicationWindow>,
    actions: Vec<(CommandId, gio::SimpleAction)>,
}

impl WindowActions {
    /// `snapshot.dialog_open` (an alert up inside the window) turns every `win.` command off
    /// through md-logic's enablement.
    fn apply(&self, snapshot: &ShellSnapshot) {
        for (id, action) in &self.actions {
            apply_one(action, action_enabled(*id, snapshot), action_state(*id, snapshot));
        }
    }
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<WindowActions>>> = const { RefCell::new(Vec::new()) };
    static APP: RefCell<Vec<(CommandId, gio::SimpleAction)>> = const { RefCell::new(Vec::new()) };
}

fn entry_for(window: &gtk::Window) -> Option<Rc<WindowActions>> {
    WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .find(|e| e.window.upgrade().is_some_and(|win| win.upcast_ref::<gtk::Window>() == window))
            .cloned()
    })
}

/// The `win.` actions of one window, each activating `dispatcher` with its `CommandId`. Every kind
/// of window carries the same actions (and the same eight menus); what is live differs by the
/// snapshot each kind publishes.
pub fn install_window_actions(window: &adw::ApplicationWindow, kind: WindowKind, dispatcher: &Rc<CommandDispatcher>) {
    let _ = kind;
    let mut actions = Vec::new();
    for id in commands_in(ActionScope::Window) {
        let Some(shape) = shape(id) else { continue };
        let action = new_action(&shape);
        let target = Rc::downgrade(dispatcher);
        action.connect_activate(move |_, parameter| {
            let Some(dispatcher) = target.upgrade() else { return };
            if let Some((id, argument)) = argument(id, parameter, || dispatcher.snapshot()) {
                dispatcher.execute(id, &argument);
            }
        });
        action.set_enabled(false);
        window.add_action(&action);
        actions.push((id, action));
    }
    let unavailable = gio::SimpleAction::new(UNAVAILABLE, None);
    unavailable.set_enabled(false);
    window.add_action(&unavailable);

    // Escape is not an accelerator on Linux (§2.2): a bubble-phase key controller runs the command
    // only when nothing below handled the key — the editor, a popover and an input method keep it.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Bubble);
    let target = Rc::downgrade(dispatcher);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        let plain = !modifiers.intersects(
            gtk::gdk::ModifierType::CONTROL_MASK
                | gtk::gdk::ModifierType::ALT_MASK
                | gtk::gdk::ModifierType::SHIFT_MASK,
        );
        match target.upgrade() {
            Some(dispatcher)
                if key == gtk::gdk::Key::Escape
                    && plain
                    && dispatcher.try_invoke(CommandId::Escape, &CommandArgument::None) =>
            {
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(keys);

    let entry = Rc::new(WindowActions { window: window.downgrade(), actions });
    // An alert opening or closing inside the window flips the snapshot's `dialog_open`: the window
    // publishes again, which re-runs the enablement (§2.2).
    window.connect_visible_dialog_notify(|window| {
        if let Some(host) = Shell::get().windows.host_for(window.upcast_ref()) {
            host.publish();
        }
    });
    // GTK 4.22 emits `destroy` only once the last reference goes, and the window's owner holds
    // one; every destroy unrealizes first, so the entry goes there.
    window.connect_unrealize(|window| {
        let window: &gtk::Window = window.upcast_ref();
        WINDOWS.with(|w| {
            w.borrow_mut().retain(|e| e.window.upgrade().is_some_and(|win| win.upcast_ref::<gtk::Window>() != window))
        });
    });
    WINDOWS.with(|w| w.borrow_mut().push(entry));
}

/// Sets the enabled flags and states of `window`'s actions — and of the `app.` actions, which are
/// app-wide and read the same from every window's snapshot — from `snapshot`.
pub fn refresh(window: &adw::ApplicationWindow, snapshot: &ShellSnapshot) {
    if let Some(entry) = entry_for(window.upcast_ref()) {
        entry.apply(snapshot);
    }
    APP.with(|app| {
        for (id, action) in app.borrow().iter() {
            apply_one(action, action_enabled(*id, snapshot), action_state(*id, snapshot));
        }
    });
}

/// The `app.` actions and every accelerator (`startup`).
pub fn install_app_actions(shell: &Rc<Shell>) {
    let mut actions = Vec::new();
    for id in commands_in(ActionScope::App) {
        let Some(shape) = shape(id) else { continue };
        let action = new_action(&shape);
        action.connect_activate(move |_, parameter| {
            if let Some((id, argument)) = argument(id, parameter, ShellSnapshot::empty) {
                crate::windows::window_manager::run_app_command(id, argument);
            }
        });
        shell.app.add_action(&action);
        actions.push((id, action));
    }
    APP.with(|app| *app.borrow_mut() = actions);
    install_accelerators(&shell.app);
}

/// `set_accels_for_action` for every registered chord of the table (§2.9).
fn install_accelerators(app: &adw::Application) {
    for spec in command_table::all() {
        let accelerators = accels::accelerators(spec);
        if accelerators.is_empty() {
            continue;
        }
        if let Some(detailed) = detailed_name(spec.id) {
            let list: Vec<&str> = accelerators.iter().map(String::as_str).collect();
            app.set_accels_for_action(&detailed, &list);
        }
    }
}

/// Each action's current `(enabled, state)` in `window` — for the self-test.
#[cfg(feature = "selftest")]
pub fn action_map(window: &adw::ApplicationWindow) -> std::collections::HashMap<String, (bool, Option<glib::Variant>)> {
    let mut out = std::collections::HashMap::new();
    if let Some(entry) = entry_for(window.upcast_ref()) {
        for (_, action) in &entry.actions {
            out.insert(format!("win.{}", action.name()), (action.is_enabled(), action.state()));
        }
    }
    APP.with(|app| {
        for (_, action) in app.borrow().iter() {
            out.insert(format!("app.{}", action.name()), (action.is_enabled(), action.state()));
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_command_is_one_unique_action_in_one_scope() {
        let mut seen: HashSet<(ActionScope, String, Option<&str>)> = HashSet::new();
        for id in CommandId::ALL {
            let Some(shape) = shape(id) else {
                assert_eq!(id, CommandId::Escape);
                continue;
            };
            assert!(
                shape.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{id:?} → {}",
                shape.name
            );
            assert!(seen.insert((shape.scope, shape.name.clone(), shape.target)), "{id:?} is not unique");
        }
        assert_eq!(seen.len(), CommandId::ALL.len() - 1);
    }

    #[test]
    fn the_scopes_are_the_designs() {
        assert_eq!(detailed_name(CommandId::New).as_deref(), Some("app.new"));
        assert_eq!(detailed_name(CommandId::Quit).as_deref(), Some("app.quit"));
        assert_eq!(detailed_name(CommandId::ExportBookLaTeX).as_deref(), Some("app.export-book-latex"));
        assert_eq!(detailed_name(CommandId::ContinueLists).as_deref(), Some("app.continue-lists"));
        assert_eq!(detailed_name(CommandId::PdfPageSize).as_deref(), Some("app.pdf-page-size"));
        assert_eq!(detailed_name(CommandId::SaveAs).as_deref(), Some("win.save-as"));
        assert_eq!(detailed_name(CommandId::ExportLaTeX).as_deref(), Some("win.export-latex"));
        assert_eq!(detailed_name(CommandId::ViewEdit).as_deref(), Some("win.view-mode::edit"));
        assert_eq!(detailed_name(CommandId::ViewSplit).as_deref(), Some("win.view-mode::split"));
        assert_eq!(detailed_name(CommandId::ViewPreview).as_deref(), Some("win.view-mode::preview"));
        assert_eq!(detailed_name(CommandId::ZenMode).as_deref(), Some("win.zen-mode"));
        assert_eq!(detailed_name(CommandId::ActivateWindow).as_deref(), Some("win.activate-window"));
        assert_eq!(detailed_name(CommandId::Undo).as_deref(), Some("win.undo"));
        assert_eq!(detailed_name(CommandId::Escape), None);
        let app: Vec<CommandId> =
            CommandId::ALL.into_iter().filter(|id| shape(*id).is_some_and(|s| s.scope == ActionScope::App)).collect();
        assert_eq!(app.len(), 23, "{app:?}");
        assert!(app.contains(&CommandId::Help) && app.contains(&CommandId::PrintBook));
        assert!(!app.contains(&CommandId::Close) && !app.contains(&CommandId::ZenMode));
    }

    #[test]
    fn radios_toggles_and_rows_have_the_right_variant_types() {
        let view = shape(CommandId::ViewSplit).unwrap();
        assert_eq!((view.parameter, view.state, view.target), (Parameter::Text, State::Text, Some("split")));
        assert_eq!(shape(CommandId::ZenMode).unwrap().state, State::Bool);
        assert_eq!(shape(CommandId::WrapSelection).unwrap().state, State::Bool);
        assert_eq!(shape(CommandId::ShowSidebar).unwrap().state, State::Bool);
        let window = shape(CommandId::ActivateWindow).unwrap();
        assert_eq!((window.parameter, window.state), (Parameter::Window, State::Window));
        let page = shape(CommandId::PdfPageSize).unwrap();
        assert_eq!((page.parameter, page.state), (Parameter::Text, State::Text));
        for id in [CommandId::Contents, CommandId::Notes, CommandId::ExportDiagramSvg] {
            assert_eq!(shape(id).unwrap().parameter, Parameter::Index, "{id:?}");
        }
        for id in [CommandId::OpenRecentEntry, CommandId::Example] {
            assert_eq!(shape(id).unwrap().parameter, Parameter::Text, "{id:?}");
        }
        assert_eq!(
            shape(CommandId::Save).unwrap(),
            ActionShape {
                scope: ActionScope::Window,
                name: "save".into(),
                target: None,
                parameter: Parameter::None,
                state: State::None
            }
        );
    }

    #[test]
    fn view_targets_map_back_to_their_commands() {
        for id in [CommandId::ViewEdit, CommandId::ViewSplit, CommandId::ViewPreview] {
            assert_eq!(view_command(view_target(id).unwrap()), Some(id));
        }
        assert_eq!(view_command("zen"), None);
    }
}
