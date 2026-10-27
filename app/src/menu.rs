//! The classic menu bar — a `gio::Menu` model shown by a `GtkPopoverMenuBar` — and the
//! application-wide accelerators. Titles here are placeholders for md-logic's command table;
//! the structure (eight menus, detailed actions, radio items, accelerators shown beside the
//! items) is what the spike proves.

use adw::prelude::*;

fn item(menu: &gio::Menu, label: &str, action: &str) {
    menu.append(Some(label), Some(action));
}

fn section(items: &[(&str, &str)]) -> gio::Menu {
    let s = gio::Menu::new();
    for (label, action) in items {
        item(&s, label, action);
    }
    s
}

pub fn model() -> gio::Menu {
    let bar = gio::Menu::new();

    let file = gio::Menu::new();
    file.append_section(None, &section(&[("_New", "app.new"), ("_Open…", "win.open")]));
    file.append_section(None, &section(&[("_Save", "win.save"), ("Save _As…", "win.save-as")]));
    file.append_section(None, &section(&[("Export as _PDF…", "win.export-pdf"), ("_Print…", "win.print")]));
    file.append_section(None, &section(&[("_Close", "win.close"), ("_Quit", "app.quit")]));
    bar.append_submenu(Some("_File"), &file);

    let edit = gio::Menu::new();
    edit.append_section(None, &section(&[("_Undo", "win.undo"), ("_Redo", "win.redo")]));
    edit.append_section(None, &section(&[("Cu_t", "win.cut"), ("_Copy", "win.copy"), ("_Paste", "win.paste")]));
    edit.append_section(None, &section(&[("_Find…", "win.find")]));
    bar.append_submenu(Some("_Edit"), &edit);

    let format = section(&[("_Bold", "win.bold"), ("_Italic", "win.italic"), ("_Code", "win.code")]);
    bar.append_submenu(Some("F_ormat"), &format);

    let view = gio::Menu::new();
    view.append_section(
        None,
        &section(&[
            ("_Edit", "win.view-mode::edit"),
            ("_Split", "win.view-mode::split"),
            ("_Preview", "win.view-mode::preview"),
        ]),
    );
    let appearance = section(&[
        ("_System", "app.color-scheme::system"),
        ("_Light", "app.color-scheme::light"),
        ("_Dark", "app.color-scheme::dark"),
    ]);
    view.append_section(None, &{
        let s = gio::Menu::new();
        s.append_submenu(Some("_Appearance"), &appearance);
        s
    });
    bar.append_submenu(Some("_View"), &view);

    bar.append_submenu(Some("_Book"), &section(&[("_New Book…", "app.new-book")]));
    bar.append_submenu(Some("_Go"), &section(&[("_Contents", "win.contents"), ("_Notes", "win.notes")]));
    bar.append_submenu(Some("_Window"), &section(&[("_Minimize", "win.minimize")]));
    bar.append_submenu(Some("_Help"), &section(&[("_About md", "app.about")]));
    bar
}

/// Application actions and every accelerator. `set_accels_for_action` takes *detailed* names,
/// so each view mode is its own entry: `win.view-mode::edit` ↔ Ctrl+1.
pub fn install(app: &adw::Application) {
    let quit = gio::SimpleAction::new("quit", None);
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            app.quit();
        }
    });
    app.add_action(&quit);

    // System / Light / Dark through libadwaita's StyleManager.
    let scheme = gio::SimpleAction::new_stateful("color-scheme", Some(glib::VariantTy::STRING), &"system".to_variant());
    scheme.connect_activate(|action, target| {
        let Some(value) = target.and_then(|t| t.get::<String>()) else { return };
        action.set_state(&value.to_variant());
        adw::StyleManager::default().set_color_scheme(match value.as_str() {
            "light" => adw::ColorScheme::ForceLight,
            "dark" => adw::ColorScheme::ForceDark,
            _ => adw::ColorScheme::Default,
        });
    });
    app.add_action(&scheme);

    for name in ["new", "new-book", "about"] {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(|a, _| eprintln!("md: app.{} (not in the spike)", a.name()));
        app.add_action(&action);
    }

    let accels: &[(&str, &[&str])] = &[
        ("app.new", &["<Control>n"]),
        ("app.quit", &["<Control>q"]),
        ("win.open", &["<Control>o"]),
        ("win.save", &["<Control>s"]),
        ("win.save-as", &["<Shift><Control>s"]),
        ("win.print", &["<Control>p"]),
        ("win.close", &["<Control>w"]),
        ("win.find", &["<Control>f"]),
        ("win.bold", &["<Control>b"]),
        ("win.italic", &["<Control>i"]),
        ("win.view-mode::edit", &["<Control>1"]),
        ("win.view-mode::split", &["<Control>2"]),
        ("win.view-mode::preview", &["<Control>3"]),
    ];
    for (action, keys) in accels {
        app.set_accels_for_action(action, keys);
    }
}
