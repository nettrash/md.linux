//! The `books` self-test group (shell-design §13.3): the no-book page; the Example Book; sidebar
//! order; selection, rename, move, delete; Alt+Up / Alt+Down; Open in New Window and back; Print
//! Book; Export Book PDF / EPUB / LaTeX.
//!
//! Everything runs against copies of the bundled Example Book in `OUT/books/`. The expectations
//! are md-core's own answers computed independently here (the listing, the reading order, the
//! renumber plan, the deletion neighbour, the compile), and the disk is read back after every
//! operation.
//!
//! Rows that need another package's real implementation detect it precisely and print
//! `SKIP books.<row> — waits for <package>` instead of recording a row while it is still the S0
//! stub: P2's file system (the S0 stub reads nothing, so the article session cannot load), P2's
//! alerts (the stub answers every prompt with Cancel at once), P1's accelerators and window
//! manager, P4's export renderer. Once those land the same rows run.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use md_core::book::book_model::{self, Book};
use md_core::book::structured_book::StructuredBook;
use md_core::book::{book_compiler, book_folder, book_paths, renumber_plan};
use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_logic::books::{BookOutputs, BookRowKind, book_sidebar_model};
use md_logic::commands::{CommandArgument, CommandId};
use md_logic::documents::Stage;
use md_logic::exec::{LocalBoxFuture, ready};
use md_logic::seams::{DocumentRegistry, FileIdentity, FileSystem, WindowId};
use md_logic::settings::{SettingsStore, settings_keys};
use md_logic::strings::{self, books};
use md_logic::windows::window_title;

use crate::book::BOOK_WINDOW_ID;
use crate::book::book_window::BookWindow;
use crate::selftest::Harness;
use crate::windows::host::WindowHost;

/// How long a window may take to appear or go.
const WINDOW_LIMIT: Duration = Duration::from_secs(10);

/// A window id no window manager hands out: the self-test's stand-in for a document window that
/// owns an article, while P1's window manager is the S0 stub.
const STAND_IN_OWNER: WindowId = WindowId(990_001);

pub async fn run(h: &Harness) {
    let shell = h.shell();
    let books_dir = h.out().join("books");
    let _ = fs::remove_dir_all(&books_dir);
    if let Err(err) = fs::create_dir_all(&books_dir) {
        h.check("setup", false, format!("cannot create {}: {err}", books_dir.display()));
        return;
    }

    // ---- no book ----
    crate::book::close_book(&shell);
    h.wait_until(WINDOW_LIMIT, || BookWindow::current().is_none()).await;
    let book = BookWindow::show(&shell);
    let window: gtk::Window = book.gtk_window().upcast();
    h.wait_until(WINDOW_LIMIT, || window.is_mapped()).await;
    h.next_frame(&window).await;
    let (content, _) = book.pages();
    let (title, message, button) = book.status();
    h.check(
        "no_book_page",
        content == "empty"
            && title == books::NO_BOOK_OPEN
            && message == books::NO_BOOK_OPEN_MESSAGE
            && button
            && window.title().as_deref() == Some(strings::BOOK),
        format!("page {content}, \"{title}\", button {button}, window \"{:?}\"", window.title()),
    );
    let rows = shell.registry.windows();
    h.check(
        "window_menu_row",
        rows.iter().any(|(id, title)| *id == BOOK_WINDOW_ID && title == strings::BOOK),
        format!("{rows:?}"),
    );
    h.screenshot("no-book").await;

    // ---- the Example Book ----
    let source = shell.examples.book_folder();
    let unpacked = shell.book.unpack_example_book_into(&books_dir.to_string_lossy());
    let second = shell.book.unpack_example_book_into(&books_dir.to_string_lossy());
    let (Ok(root), Ok(second)) = (unpacked, second) else {
        h.check("example_book_unpacked", false, "the copy failed");
        return;
    };
    let same = tree(&source) == tree(Path::new(&root)) && tree(&source) == tree(Path::new(&second));
    h.check(
        "example_book_unpacked",
        same && root.ends_with("/Example Book") && second.ends_with("/Example Book 2") && !tree(&source).is_empty(),
        format!("{root}, {second}"),
    );

    shell.book.open_folder(&root);
    let Some(listed) = book_model::load(&root) else {
        h.check("book_opens", false, format!("md-core cannot list {root}"));
        return;
    };
    let order: Vec<String> = book_model::reading_order(&listed).into_iter().map(|a| a.path).collect();
    let navigator = book.navigator();
    let session = book.session();
    let files_landed = shell.files.directory_exists("/");
    let editing_title = files_landed.then(|| session.title());
    let expected_title = window_title::for_book(Some(listed.name()), editing_title.as_deref());
    h.check(
        "book_opens",
        book.pages().0 == "book"
            && shell.book.has_book()
            && shell.book.folder().as_deref() == Some(root.as_str())
            && navigator.root().as_deref() == Some(listed.root.as_str())
            && window.title().as_deref() == Some(expected_title.as_str()),
        format!("page {}, window \"{:?}\", expected \"{expected_title}\"", book.pages().0, window.title()),
    );

    // ---- the sidebar ----
    let sidebar = book.sidebar();
    let expected: Vec<(BookRowKind, String, bool)> = book_sidebar_model::rows(Some(&listed))
        .into_iter()
        .map(|row| {
            let selectable = row.is_selectable();
            (row.kind, row.title, selectable)
        })
        .collect();
    let shown = sidebar.shown_rows();
    let shown_articles: Vec<&String> = shown.iter().filter(|r| r.2).map(|r| &r.1).collect();
    let order_names: Vec<String> = book_model::reading_order(&listed).iter().map(|a| a.name().to_string()).collect();
    h.check(
        "sidebar_order",
        shown == expected && shown_articles.iter().map(|s| s.as_str()).eq(order_names.iter().map(String::as_str)),
        format!("{:?}", shown.iter().map(|r| r.1.as_str()).collect::<Vec<_>>()),
    );
    let first = order.first().cloned().unwrap_or_default();
    h.check(
        "first_article_selected",
        navigator.selection().as_deref() == Some(first.as_str())
            && sidebar.selected_path().as_deref() == Some(first.as_str())
            && shell.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).as_deref() == Some("01-Preface.md"),
        format!("{:?} / {:?}", navigator.selection(), sidebar.selected_path()),
    );

    // ---- the article ----
    if files_landed {
        let text = book_folder::read_article(&first).unwrap_or_default();
        h.check(
            "selection_opens_article",
            session.stage() == Stage::Editing(first.clone()) && session.text() == text && book.pages().1 == "editing",
            format!("{:?}, {} chars, page {}", session.stage(), session.text().len(), book.pages().1),
        );
    } else {
        skip("selection_opens_article", "P2 (StdFileSystem is the S0 stub, so no article loads)");
        // What the pane shows for an article it cannot read, which is what the stub makes of it.
        let (title, message, button) = book.status();
        h.check(
            "unreadable_page",
            matches!(session.stage(), Stage::Unreadable(_))
                && book.pages().1 == "status"
                && title == books::UNREADABLE_TITLE
                && message == books::unreadable_message("Preface")
                && !button,
            format!("{:?}, \"{title}\" / \"{message}\"", session.stage()),
        );
    }
    h.screenshot("example-book").await;

    if files_landed {
        let before = fs::metadata(&first).map(|m| m.ino()).ok();
        let edited = format!("{}\nA line written in the book.\n", session.text());
        session.edit(&edited);
        let saved = h
            .wait_until(Duration::from_secs(4), || fs::read_to_string(&first).ok().as_deref() == Some(edited.as_str()))
            .await;
        let after = fs::metadata(&first).map(|m| m.ino()).ok();
        h.check(
            "edit_autosaves_in_place",
            saved && before.is_some() && before == after && !session.has_unsaved_changes(),
            format!("saved {saved}, inode {before:?} → {after:?}"),
        );
    } else {
        skip("edit_autosaves_in_place", "P2 (StdFileSystem)");
    }

    // ---- stepping ----
    let dispatcher = book.dispatcher();
    let (previous, next) = book.chevrons();
    let at = |index: usize| order.get(index).cloned();
    let snapshot = book.snapshot();
    let entry_ok = !snapshot.can_previous && snapshot.can_next && !previous.is_sensitive() && next.is_sensitive();
    dispatcher.execute(CommandId::NextArticle, &CommandArgument::None);
    let by_command = navigator.selection() == at(1);
    dispatcher.reset_debounce();
    next.emit_clicked();
    let by_chevron = navigator.selection() == at(2);
    previous.emit_clicked();
    let back = navigator.selection() == at(1) && sidebar.selected_path() == at(1);
    h.check(
        "step_next_previous",
        entry_ok && by_command && by_chevron && back,
        format!("at the front {entry_ok}, Go ▸ Next {by_command}, chevrons {by_chevron}/{back}"),
    );
    dispatcher.execute(CommandId::PreviousArticle, &CommandArgument::None);
    let accels = shell.app.accels_for_action("win.next-article");
    if !h.input() {
        skip("alt_down_up", "the input tier (--no-input)");
    } else if accels.is_empty() {
        skip("alt_down_up", "P1 (no accelerator on win.next-article yet)");
    } else {
        h.focus(&window).await;
        h.key("alt+Down").await;
        let down = navigator.selection() == at(1);
        h.key("alt+Up").await;
        let up = navigator.selection() == at(0);
        h.check("alt_down_up", down && up, format!("accels {accels:?}, Alt+Down {down}, Alt+Up {up}"));
    }

    // A real click on a sidebar row selects it (the list's own selection, no per-row handler).
    if h.input() {
        let target = at(2).unwrap_or_default();
        let clicked = match sidebar.row_widget(&target) {
            Some(row) => {
                let y = f64::from(row.height()) / 2.0;
                h.focus(&window).await;
                h.click(&window, &row, 40.0, y).await
            }
            None => false,
        };
        let selected = h.wait_until(Duration::from_secs(2), || navigator.selection().as_deref() == Some(&target)).await;
        h.check("click_selects_row", clicked && selected, format!("{:?}", navigator.selection()));

        // The secondary click opens the management menu; Move Up / Down are dead at the ends.
        let preface = at(0).unwrap_or_default();
        let mut ends = (true, true);
        let opened = match sidebar.row_widget(&preface) {
            Some(row) => {
                // The harness clicks button 1 there (selecting the row), then button 3 in place.
                let y = f64::from(row.height()) / 2.0;
                let ok = h.click(&window, &row, 40.0, y).await && h.xdotool(&["click", "3"]).await.0;
                let shown = h.wait_until(Duration::from_secs(2), || popover_open(&row)).await;
                // Read while the menu is up: the shared actions go back to live when it closes.
                ends = sidebar.move_enabled();
                h.screenshot("context-menu").await;
                h.key("Escape").await;
                let closed = h.wait_until(Duration::from_secs(2), || !popover_open(&row)).await;
                ok && shown && closed
            }
            None => false,
        };
        h.check(
            "context_menu",
            opened && ends == (false, false) && sidebar.move_enabled() == (true, true),
            format!("menu {opened}, Move Up / Down live at the ends {ends:?}"),
        );
    } else {
        skip("click_selects_row", "the input tier (--no-input)");
        skip("context_menu", "the input tier (--no-input)");
    }

    // ---- management: create, rename, reorder, delete ----
    navigator.create_chapter("04-Appendix");
    let chapter = book_paths::combine(&listed.root, "04-Appendix");
    let shown = sidebar.shown_rows();
    let header_at = shown.iter().position(|r| r.0 == BookRowKind::ChapterHeader && r.1 == "04-Appendix");
    h.check(
        "new_chapter",
        Path::new(&chapter).is_dir()
            && header_at.is_some_and(|i| shown.get(i + 1).is_some_and(|r| r.0 == BookRowKind::NewArticle)),
        format!("{chapter}, header row {header_at:?}"),
    );
    navigator.create_article("Glossary", &chapter);
    let glossary = book_paths::combine(&chapter, "Glossary.md");
    h.check(
        "new_article",
        fs::read_to_string(&glossary).ok().as_deref() == Some("# Glossary\n")
            && navigator.selection().as_deref() == Some(glossary.as_str())
            && sidebar.selected_path().as_deref() == Some(glossary.as_str()),
        format!("{:?}", navigator.selection()),
    );

    let getting_started = book_paths::combine(&listed.root, "02-Getting Started");
    let editor_article = book_paths::combine(&getting_started, "01-The Editor.md");
    navigator.select(Some(&editor_article));
    if !h.input() {
        skip("rename_prompt", "the input tier (--no-input)");
    } else if !prompt_appears(h, &book, "book.rename", &editor_article).await {
        skip("rename_prompt", "P2 (AdwAlerts is the S0 stub and answers Cancel)");
    } else {
        h.key("ctrl+a").await;
        h.type_text("The Writing Desk").await;
        h.key("Return").await;
        let renamed = book_paths::combine(&getting_started, "01-The Writing Desk.md");
        let done = h.wait_until(Duration::from_secs(3), || Path::new(&renamed).is_file()).await;
        h.check("rename_prompt", done, renamed.to_string());
        close_dialog(&book);
    }
    if Path::new(&editor_article).exists() {
        navigator.perform_rename(&editor_article, "The Writing Desk");
    }
    let renamed = book_paths::combine(&getting_started, "01-The Writing Desk.md");
    h.check(
        "rename_keeps_prefix",
        Path::new(&renamed).is_file()
            && !Path::new(&editor_article).exists()
            && navigator.selection().as_deref() == Some(renamed.as_str()),
        format!("{:?}: {:?}", names(&getting_started), navigator.selection()),
    );

    // Move Down through the context menu's action: the group is renumbered exactly as md-core plans.
    let siblings = names(&getting_started);
    let plan = renumber_plan::plan(&siblings, 0, 1);
    let expected = renamed_by(&siblings, &plan);
    activate(&book, sidebar_action(crate::book::sidebar::MOVE_DOWN), &renamed);
    let moved = book_paths::combine(&getting_started, "02-The Writing Desk.md");
    let chapter_rows: Vec<String> = sidebar
        .shown_rows()
        .into_iter()
        .skip_while(|r| r.1 != "02-Getting Started")
        .skip(1)
        .take_while(|r| r.0 == BookRowKind::ChapterArticle)
        .map(|r| r.1)
        .collect();
    h.check(
        "move_down_renumbers",
        names(&getting_started) == expected
            && expected == ["01-Saving and Export.md", "02-The Writing Desk.md"]
            && chapter_rows == ["01-Saving and Export", "02-The Writing Desk"]
            && navigator.selection().as_deref() == Some(moved.as_str()),
        format!("{:?}, rows {chapter_rows:?}, {:?}", names(&getting_started), navigator.selection()),
    );

    // Move Up on a chapter: the chapter group is renumbered in the root as md-core plans, and the
    // selection follows its article into whatever its chapter is now called.
    let chapters: Vec<String> =
        navigator.book().map(|b| b.chapters.iter().map(|c| c.name().to_string()).collect()).unwrap_or_default();
    let chapter_plan = renumber_plan::plan(&chapters, 1, 0);
    let mut expected_root = renamed_by(&chapters, &chapter_plan);
    expected_root.push("01-Preface.md".to_string());
    expected_root.sort();
    let new_name = |old: &str| {
        chapter_plan.iter().find(|pair| pair.from == old).map_or_else(|| old.to_string(), |pair| pair.to.clone())
    };
    let going_further = book_paths::combine(&listed.root, "03-Going Further");
    activate(&book, sidebar_action(crate::book::sidebar::MOVE_UP), &going_further);
    let followed = book_paths::combine(
        &book_paths::combine(&listed.root, &new_name("02-Getting Started")),
        "02-The Writing Desk.md",
    );
    h.check(
        "move_chapter_up",
        names(&listed.root) == expected_root
            && new_name("03-Going Further") != "03-Going Further"
            && Path::new(&followed).is_file()
            && navigator.selection().as_deref() == Some(followed.as_str()),
        format!("{:?}, {:?}", names(&listed.root), navigator.selection()),
    );

    // Delete: the selection falls to the neighbour md-core names, computed before the delete.
    let before = navigator.book().unwrap_or_else(|| Book::empty(&listed.root));
    let neighbour = book_folder::deletion_neighbor(&before, &followed);
    navigator.perform_delete(&followed);
    h.check(
        "delete_picks_neighbour",
        !Path::new(&followed).exists() && neighbour.is_some() && navigator.selection() == neighbour,
        format!("neighbour {neighbour:?}, selection {:?}", navigator.selection()),
    );
    // The appendix chapter (renumbered by the move above), with the article in it: the folder goes
    // with everything inside, and the selection leaves it for md-core's neighbour.
    let current = navigator.book().unwrap_or_else(|| Book::empty(&listed.root));
    let appendix = current
        .chapters
        .iter()
        .find(|c| c.name().ends_with("-Appendix"))
        .map(|c| (c.path.clone(), c.name().to_string()));
    let (appendix, appendix_name) = appendix.unwrap_or_default();
    let neighbour = book_folder::deletion_neighbor(&current, &appendix);
    let existed = Path::new(&appendix).is_dir() && !names(&appendix).is_empty();
    navigator.perform_delete(&appendix);
    h.check(
        "delete_chapter",
        existed
            && !Path::new(&appendix).exists()
            && !sidebar.shown_rows().iter().any(|r| r.1 == appendix_name)
            && neighbour.is_some()
            && navigator.selection() == neighbour,
        format!("{appendix} ({existed}), selection {:?}", navigator.selection()),
    );
    h.screenshot("after-management").await;

    // ---- the sidebar's width and visibility ----
    let paned = book.paned();
    paned.set_position(100);
    let low = paned.position();
    paned.set_position(500);
    let high = paned.position();
    paned.set_position(260);
    let mid = paned.position();
    h.check("sidebar_clamped", (low, high, mid) == (200, 320, 260), format!("{low} / {high} / {mid}"));
    dispatcher.reset_debounce();
    dispatcher.execute(CommandId::ShowSidebar, &CommandArgument::None);
    let hidden = !book.snapshot().sidebar_open && !sidebar.widget().is_visible();
    dispatcher.reset_debounce();
    dispatcher.execute(CommandId::ShowSidebar, &CommandArgument::None);
    let shown_again = book.snapshot().sidebar_open && sidebar.widget().is_visible();
    h.check("show_sidebar", hidden && shown_again, format!("hidden {hidden}, shown {shown_again}"));

    // ---- the view mode: md.bookViewMode, app-wide ----
    if files_landed {
        dispatcher.reset_debounce();
        dispatcher.execute(CommandId::ViewPreview, &CommandArgument::None);
        let stored = shell.settings.get_string(settings_keys::BOOK_VIEW_MODE);
        let toggled = book.toggled_mode();
        dispatcher.reset_debounce();
        if let Some(split) = book.mode_button(ViewMode::Split) {
            split.set_active(true);
        }
        let back = shell.settings.get_string(settings_keys::BOOK_VIEW_MODE);
        h.check(
            "view_mode_stored",
            stored.as_deref() == Some("preview")
                && toggled == Some(ViewMode::Preview)
                && back.as_deref() == Some("split"),
            format!("{stored:?} / {toggled:?} / {back:?}"),
        );
    } else {
        skip("view_mode_stored", "P2 (no article is editable, so View is disabled)");
    }

    // ---- Open in New Window: the handoff and the reclaim ----
    if files_landed {
        open_in_window(h, &book, &shell).await;
    } else {
        skip("handoff_and_reclaim", "P2 (StdFileSystem)");
        // The mark the document window claims is set even when nothing can be edited here.
        let target = navigator.selection().unwrap_or_default();
        activate(&book, sidebar_action(crate::book::sidebar::OPEN_IN_WINDOW), &target);
        let marked = shell.book.article_opens().borrow_mut().claim_open(&target);
        h.check("open_in_window_marks", marked, target.clone());
        window.present();
    }

    // ---- the whole-book outputs: flush, list, compile ----
    outputs(h, &book, &shell, files_landed).await;

    // ---- Close Book forgets it; the folder stays; the next open remembers the article ----
    let last = shell.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).unwrap_or_default();
    crate::book::close_book(&shell);
    let gone = h.wait_until(WINDOW_LIMIT, || BookWindow::current().is_none()).await;
    h.check(
        "close_book_forgets",
        gone && !shell.book.has_book()
            && shell.settings.get_string(settings_keys::BOOK_BOOKMARK).is_none()
            && Path::new(&root).is_dir()
            && shell.settings.get_string(settings_keys::BOOK_WINDOW_SIZE).is_some(),
        format!("window gone {gone}, bookmark {:?}", shell.settings.get_string(settings_keys::BOOK_BOOKMARK)),
    );
    shell.book.open_folder(&root);
    let book = BookWindow::show(&shell);
    let remembered = book_folder::path_in(&listed.root, &last);
    h.check(
        "reopen_remembers_article",
        !last.is_empty() && book.navigator().selection() == remembered,
        format!("{last} → {:?}", book.navigator().selection()),
    );

    // A book on a drive that is gone: No Book Open, and the setting is kept for when it is back.
    let missing = books_dir.join("Unplugged Book").to_string_lossy().into_owned();
    shell.book.open_folder(&missing);
    let (content, _) = book.pages();
    h.check(
        "inaccessible_book_keeps_setting",
        content == "empty"
            && shell.book.has_book()
            && !shell.book.is_accessible()
            && shell.book.folder().as_deref() == Some(missing.as_str()),
        format!("page {content}, folder {:?}", shell.book.folder()),
    );
    crate::book::close_book(&shell);
    h.wait_until(WINDOW_LIMIT, || BookWindow::current().is_none()).await;
}

/// Open in New Window: the article is saved and handed off before the document window opens, the
/// pane shows the handoff page, and the article comes back when its owner lets go.
async fn open_in_window(h: &Harness, book: &Rc<BookWindow>, shell: &Rc<crate::app::Shell>) {
    let navigator = book.navigator();
    let session = book.session();
    let Some(target) = navigator.selection() else {
        h.check("handoff_and_reclaim", false, "nothing selected");
        return;
    };
    let canonical = shell.identity.canonical(&target);
    let p1_landed = !shell.app.accels_for_action("win.next-article").is_empty();
    if !p1_landed {
        // P1's window manager is the S0 stub: a stand-in owner takes the article as the document
        // window would, so the handoff stays put; letting go of it must give the article back.
        skip("show_window_presents_owner", "P1 (WindowManager::open_path is the S0 stub)");
        shell.registry.register(STAND_IN_OWNER, &canonical);
    }
    activate(book, sidebar_action(crate::book::sidebar::OPEN_IN_WINDOW), &target);
    let owned = h.wait_until(WINDOW_LIMIT, || shell.registry.owning(&canonical).is_some()).await;
    // The document window came forward; the handoff page is the Book window's.
    book.gtk_window().present();
    h.sleep(Duration::from_millis(300)).await;
    let (title, _, button) = book.status();
    let handed_off = owned
        && session.stage() == Stage::Handoff(target.clone())
        && book.pages().1 == "status"
        && title == books::HANDOFF_TITLE
        && button;
    h.screenshot("handoff").await;
    let owner = shell.registry.owning(&canonical);
    if p1_landed {
        let presented = match owner.and_then(|id| shell.windows.host(id)) {
            Some(host) => {
                book.gtk_window().present();
                h.sleep(Duration::from_millis(300)).await;
                book.status_button().emit_clicked();
                let window = host.window();
                let active = h.wait_until(WINDOW_LIMIT, || window.is_active()).await;
                window.close();
                active
            }
            None => false,
        };
        h.check("show_window_presents_owner", presented, format!("owner {owner:?}"));
    } else {
        // `remove`, not `unregister`: the stand-in also got a Window-menu row by registering.
        shell.registry.remove(STAND_IN_OWNER);
    }
    let reclaimed = h.wait_until(WINDOW_LIMIT, || session.stage() == Stage::Editing(target.clone())).await;
    book.gtk_window().present();
    h.check(
        "handoff_and_reclaim",
        handed_off && reclaimed && book.pages().1 == "editing",
        format!("handoff {handed_off} (\"{title}\"), reclaimed {reclaimed}"),
    );
}

/// Print Book and Export Book: every output flushes the article through the app's hub, lists the
/// book afresh and compiles it — checked through a recording sink, against md-core's own compile
/// of the folder. The render into files is P4's pipeline.
async fn outputs(h: &Harness, book: &Rc<BookWindow>, shell: &Rc<crate::app::Shell>, files_landed: bool) {
    let session = book.session();
    let sink = Rc::new(RecordingSink::default());
    let output = shell.book.book_output(sink.clone());
    let alerts = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&alerts);
    output.alert_requested().connect(move |alert: &(String, String)| seen.borrow_mut().push(alert.clone()));

    // An edit still waiting for its autosave must reach the compile: the hub flushes first.
    let pending = files_landed.then(|| {
        let edited = format!("{}\nWritten just before printing.\n", session.text());
        session.edit(&edited);
        edited
    });
    output.print().await;
    output.export_pdf().await;
    output.export_epub().await;
    output.export_latex().await;

    let listed = shell.book.folder().and_then(|folder| book_model::load(&folder)).unwrap_or_else(|| Book::empty("/"));
    let source = book_compiler::compile_book_source(&listed, book_folder::read_article).unwrap_or_default();
    let structured = book_compiler::read_structured_book(&listed, book_folder::read_article).ok();
    let title = book_compiler::title(&listed);
    let page = *PageSize::named(shell.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref());
    let calls = sink.calls.borrow();
    let flushed = pending.is_none_or(|edited| source.contains("Written just before printing.") && !edited.is_empty());
    let expected = vec![
        Recorded::Print(source.clone(), title.clone()),
        Recorded::Pdf(source.clone(), title.clone(), page.id),
        Recorded::Epub(structured.clone()),
        Recorded::LaTeX(structured.clone()),
    ];
    h.check(
        "outputs_flush_and_compile",
        *calls == expected && flushed && alerts.borrow().is_empty() && !source.is_empty(),
        format!(
            "{} calls, flushed {flushed}, alerts {:?}, {} chapters",
            calls.len(),
            alerts.borrow(),
            structured.as_ref().map_or(0, |b| b.sections.len())
        ),
    );
    if !files_landed {
        skip("outputs_flush_pending_edit", "P2 (no article is editable)");
    }
    if crate::book::outputs_wired(shell) {
        skip("print_and_export_book_files", "a self-test answer for P4's save dialog (not written yet)");
    } else {
        skip("print_and_export_book_files", "P4 (WindowExports::book_outputs is the S0 stub)");
    }
}

/// What the recording sink saw.
#[derive(Clone, Debug, PartialEq)]
enum Recorded {
    Print(String, String),
    Pdf(String, String, &'static str),
    Epub(Option<StructuredBook>),
    LaTeX(Option<StructuredBook>),
}

/// A `BookOutputs` that remembers each call instead of rendering.
#[derive(Default)]
struct RecordingSink {
    calls: RefCell<Vec<Recorded>>,
}

impl BookOutputs for RecordingSink {
    fn export_pdf(&self, source: String, title: String, page_size: PageSize) -> LocalBoxFuture<'static, ()> {
        self.calls.borrow_mut().push(Recorded::Pdf(source, title, page_size.id));
        ready(())
    }

    fn print(&self, source: String, title: String) -> LocalBoxFuture<'static, ()> {
        self.calls.borrow_mut().push(Recorded::Print(source, title));
        ready(())
    }

    fn export_epub(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.calls.borrow_mut().push(Recorded::Epub(Some(book)));
        ready(())
    }

    fn export_latex(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.calls.borrow_mut().push(Recorded::LaTeX(Some(book)));
        ready(())
    }
}

/// A row that waits for another package: printed, not recorded.
fn skip(name: &str, waits_for: &str) {
    println!("SKIP books.{name} — waits for {waits_for}");
}

/// `book.<name>`.
fn sidebar_action(name: &str) -> String {
    format!("book.{name}")
}

/// Activates a `book.` action with a path target, as the context menu's row does.
fn activate(book: &BookWindow, action: String, path: &str) {
    let window = book.gtk_window();
    if let Err(err) = WidgetExt::activate_action(&window, &action, Some(&path.to_variant())) {
        eprintln!("md selftest: {action}: {err}");
    }
}

/// Activates a prompting action and reports whether a dialog came up for it.
async fn prompt_appears(h: &Harness, book: &BookWindow, action: &str, path: &str) -> bool {
    let window = book.gtk_window();
    h.focus(window.upcast_ref()).await;
    activate(book, action.to_string(), path);
    h.wait_until(Duration::from_secs(2), || window.visible_dialog().is_some()).await
}

/// Closes whatever dialog is still up, so a failed row cannot block the next.
fn close_dialog(book: &BookWindow) {
    if let Some(dialog) = book.gtk_window().visible_dialog() {
        dialog.force_close();
    }
}

/// Whether a popover parented to `row` is showing.
fn popover_open(row: &gtk::ListBoxRow) -> bool {
    let mut child = row.first_child();
    while let Some(widget) = child {
        if widget.is::<gtk::PopoverMenu>() && widget.is_visible() {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

/// `names` after the renames of `plan`, sorted — what the folder should list once it is applied.
fn renamed_by(names: &[String], plan: &[renumber_plan::RenamePair]) -> Vec<String> {
    let mut out: Vec<String> = names
        .iter()
        .map(|name| plan.iter().find(|pair| pair.from == *name).map_or_else(|| name.clone(), |pair| pair.to.clone()))
        .collect();
    out.sort();
    out
}

/// The entry names of a folder, sorted.
fn names(folder: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Every entry under `root`, relative, with the bytes of each file — for comparing two copies.
fn tree(root: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let relative = path.strip_prefix(root).map(Path::to_path_buf).unwrap_or_default();
            if path.is_dir() {
                out.push((relative, None));
                stack.push(path);
            } else {
                out.push((relative, fs::read(&path).ok()));
            }
        }
    }
    out.sort();
    out
}
