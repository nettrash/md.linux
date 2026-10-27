//! Port of md.win `tests/Md.App.Logic.Tests/ActivationRouterTests.cs`.
//!
//! shell-design.md §1.2's routing table, row by row, plus the ownership resolution §1.2's prose
//! adds ("a path already open activates that window instead of opening a second one"). md.win's
//! activations are WinRT's; these are `GApplication`'s (see `activation_description`): `activate`
//! is a launch, `open` a file activation, and the rows that were Windows' command-line rules pin
//! the Linux ones.

use md_logic::activation::activation_router::{self, classify};
use md_logic::activation::{ActivationAction, ActivationDescription, ActivationItem, ActivationKind};
use md_logic::testing::*;

fn route(description: &ActivationDescription) -> Vec<ActivationAction> {
    activation_router::route(description)
}

fn open(path: &str) -> ActivationAction {
    ActivationAction::OpenPath(path.to_string())
}

// ── the table: File ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_file_activation_opens_one_document_per_file_in_the_order_the_shell_gave_them() {
    let actions = route(&ActivationDescription::for_files(
        vec![ActivationItem::file("/Docs/a.md"), ActivationItem::file("/Docs/b.markdown")],
        true,
        false,
    ));

    assert_eq!(actions, [open("/Docs/a.md"), open("/Docs/b.markdown")]);
}

#[test]
fn a_text_pack_file_imports_rather_than_opens() {
    let actions =
        route(&ActivationDescription::for_files(vec![ActivationItem::file("/Docs/notes.textpack")], true, false));
    assert_eq!(actions, [ActivationAction::ImportTextPack("/Docs/notes.textpack".into())]);
}

#[test]
fn a_text_bundle_folder_imports_and_every_other_folder_is_ignored() {
    let actions = route(&ActivationDescription::for_files(
        vec![
            ActivationItem::folder("/Docs/Notes.textbundle"),
            ActivationItem::folder("/Docs/Pictures"),
            ActivationItem::file("/Docs/a.md"),
        ],
        true,
        false,
    ));

    assert_eq!(
        actions,
        [ActivationAction::ImportTextBundleFolder("/Docs/Notes.textbundle".into()), open("/Docs/a.md")]
    );
}

#[test]
fn the_bundle_suffix_is_matched_case_insensitively_and_only_on_the_name() {
    // A document that merely lives inside a bundle folder is still a document.
    let actions = route(&ActivationDescription::for_files(
        vec![ActivationItem::folder("/Docs/NOTES.TEXTBUNDLE"), ActivationItem::file("/Docs/Notes.textbundle/text.md")],
        true,
        false,
    ));

    assert!(matches!(actions[0], ActivationAction::ImportTextBundleFolder(_)));
    assert!(matches!(actions[1], ActivationAction::OpenPath(_)));
}

#[test]
fn a_file_activation_of_nothing_but_ignored_folders_still_gives_the_first_launch_a_window() {
    let first = route(&ActivationDescription::for_files(vec![ActivationItem::folder("/Docs/Pictures")], true, false));
    assert_eq!(first, [ActivationAction::OpenUntitled]);

    // A forwarded activation that asked for nothing does nothing: the running instance already has
    // windows.
    let forwarded =
        route(&ActivationDescription::for_files(vec![ActivationItem::folder("/Docs/Pictures")], false, false));
    assert!(forwarded.is_empty());
}

// ── the table: Launch with arguments ────────────────────────────────────────────────────────────

#[test]
fn launch_with_arguments_opens_one_document_per_argument() {
    let actions = route(&ActivationDescription::for_launch(&["a.md", "/My Docs/b.md"], true, false));
    assert_eq!(actions, [open("a.md"), open("/My Docs/b.md")]);
}

#[test]
fn a_text_pack_on_the_command_line_imports_like_a_double_click() {
    let actions = route(&ActivationDescription::for_launch(&["/Docs/notes.textpack"], true, false));
    assert_eq!(actions, [ActivationAction::ImportTextPack("/Docs/notes.textpack".into())]);
}

/// md.win drops a leading `*.exe` token (an unpackaged run can see its own image path). LINUX:
/// argv[0] is never passed — `GApplication` strips it — so there is no executable token, and a
/// document that happens to be named `*.exe` is a document.
#[test]
fn the_executable_is_not_a_document() {
    let actions = route(&ActivationDescription::for_launch(&["a.md"], true, false));
    assert_eq!(actions, [open("a.md")]);
    let actions = route(&ActivationDescription::for_launch(&["/Downloads/tool.exe", "a.md"], true, false));
    assert_eq!(actions, [open("/Downloads/tool.exe"), open("a.md")]);
}

/// LINUX: only `-` starts a switch; a `/` starts every absolute path.
#[test]
fn a_command_line_carrying_a_switch_is_not_a_document_list() {
    // A switch's operand (an output folder, say) is not a document; the app takes that mode over
    // before routing. Opening it as a document would be a bug with a visible window.
    assert_eq!(
        route(&ActivationDescription::for_launch(&["--spike", "/tmp/out"], true, false)),
        [ActivationAction::OpenUntitled]
    );
    assert_eq!(route(&ActivationDescription::for_launch(&["--spike"], true, true)), [ActivationAction::RestoreSession]);
    assert_eq!(route(&ActivationDescription::for_launch(&["/tmp/out"], true, false)), [open("/tmp/out")]);
}

// ── the table: plain Launch ─────────────────────────────────────────────────────────────────────

#[test]
fn a_plain_first_launch_restores_the_session_when_there_is_one() {
    assert_eq!(route(&ActivationDescription::for_launch(&[], true, true)), [ActivationAction::RestoreSession]);
}

#[test]
fn a_plain_first_launch_with_nothing_to_restore_opens_one_untitled_window() {
    assert_eq!(route(&ActivationDescription::for_launch(&[""], true, false)), [ActivationAction::OpenUntitled]);
}

#[test]
fn a_plain_redirected_launch_always_opens_an_untitled_window_and_never_restores() {
    // What clicking the Dock icon gives on the Mac: a new document, not the old session back. On
    // Linux, `md` run again while md runs: D-Bus forwards `activate` to the primary instance.
    assert_eq!(route(&ActivationDescription::for_launch(&[], false, true)), [ActivationAction::OpenUntitled]);
}

/// LINUX: md.win's theory runs Protocol, StartupTask and Other; `GApplication` has no protocol or
/// startup-task activation, so `Other` is the one kind left to route as a launch.
#[test]
fn every_other_kind_is_treated_as_a_launch() {
    assert_eq!(
        route(&ActivationDescription::for_other(ActivationKind::Other, true, false)),
        [ActivationAction::OpenUntitled]
    );
    assert_eq!(
        route(&ActivationDescription::for_other(ActivationKind::Other, true, true)),
        [ActivationAction::RestoreSession]
    );
    assert_eq!(
        route(&ActivationDescription::for_other(ActivationKind::Launch, true, true)),
        [ActivationAction::RestoreSession]
    );
}

// ── command-line splitting ──────────────────────────────────────────────────────────────────────

/// md.win's `ArgumentsAreSplitTheWayWindowsSplitsThem` pins `CommandLineToArgvW`. LINUX: argv
/// arrives split by the shell, and md splits nothing — spaces, quotes and backslashes are ordinary
/// characters of the token they are in; only empty tokens are skipped.
#[test]
fn arguments_are_split_the_way_windows_splits_them() {
    let rows: [(&[&str], &[&str]); 7] = [
        (&[], &[]),
        (&[""], &[]),
        (&["a.md"], &["a.md"]),
        (&["a.md", "", "b.md"], &["a.md", "b.md"]),
        (&["/My Docs/a.md"], &["/My Docs/a.md"]),
        (&["C:\\Docs\\a.md"], &["C:\\Docs\\a.md"]),
        (&["a\"b", "\"quoted\""], &["a\"b", "\"quoted\""]),
    ];
    for (arguments, expected) in rows {
        assert_eq!(
            ActivationDescription::for_launch(arguments, true, false).argument_paths(),
            expected,
            "{arguments:?}"
        );
    }
}

/// md.win pins the `"C:\My Docs\" b.md` trap (one argument). LINUX: a trailing backslash is a
/// character of the name, and the shell, not md, decided where the token ends.
#[test]
fn a_trailing_backslash_before_a_closing_quote_escapes_it_the_way_command_line_to_argv_does() {
    let description = ActivationDescription::for_launch(&["/My Docs\\", "b.md"], true, false);
    assert_eq!(description.argument_paths(), ["/My Docs\\", "b.md"]);
}

/// LINUX: the `.desktop` file's `Exec=md %U` hands URIs; a `file://` URI is its path, decoded, and
/// any other scheme names nothing md can edit in place.
#[test]
fn a_file_uri_is_its_path_and_another_scheme_is_not_a_document() {
    let description = ActivationDescription::for_launch(
        &[
            "file:///home/n/My%20Docs/a.md",
            "file://localhost/home/n/b.md",
            "FILE:///home/n/%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82.md",
            "file:/home/n/c.md",
            "sftp://host/home/n/d.md",
            "file://otherhost/home/n/e.md",
            "file:///home/n/bad%zz.md",
            "file:///home/n/nul%00.md",
            "notes:v2.md",
        ],
        true,
        false,
    );
    assert_eq!(
        description.argument_paths(),
        ["/home/n/My Docs/a.md", "/home/n/b.md", "/home/n/Привет.md", "/home/n/c.md", "notes:v2.md"]
    );

    assert_eq!(
        ActivationItem::from_location("file:///home/n/x.textbundle", true),
        Some(ActivationItem::folder("/home/n/x.textbundle"))
    );
    assert_eq!(ActivationItem::from_location("trash:///a.md", false), None);
    assert_eq!(ActivationItem::from_location("", false), None);
    assert_eq!(ActivationItem::from_location("/home/n/a.md", false), Some(ActivationItem::file("/home/n/a.md")));
}

// ── ownership resolution ────────────────────────────────────────────────────────────────────────

/// LINUX: md.win's second spelling differs in case, which on ext4 is another file; a dot segment
/// is a second spelling of one file.
#[test]
fn a_path_already_open_becomes_a_focus_of_that_window() {
    let registry = FakeDocumentRegistry::new();
    let identity = FakeFileIdentity::new();
    let owner = WindowId(7);
    registry.register(owner, &identity.canonical("/Docs/a.md"));

    let resolved = activation_router::resolve([open("/Docs/./a.md"), open("/Docs/b.md")], &registry, &identity);

    assert_eq!(resolved, [ActivationAction::Focus(owner), open("/Docs/b.md")]);
}

#[test]
fn two_spellings_of_one_unopened_file_in_one_activation_give_one_window() {
    let identity = FakeFileIdentity::new();
    identity.alias("/Docs/LINK.md", "/Docs/a.md");

    let resolved = activation_router::resolve(
        [open("/Docs/a.md"), open("/Docs/LINK.md")],
        &FakeDocumentRegistry::new(),
        &identity,
    );

    assert_eq!(resolved, [open("/Docs/a.md")]);
}

#[test]
fn one_window_is_focused_once_even_when_two_of_its_spellings_arrive() {
    let registry = FakeDocumentRegistry::new();
    let identity = FakeFileIdentity::new();
    identity.alias("/Docs/LINK.md", "/Docs/a.md");
    let owner = WindowId(8);
    registry.register(owner, &identity.canonical("/Docs/a.md"));

    let resolved = activation_router::resolve([open("/Docs/a.md"), open("/Docs/LINK.md")], &registry, &identity);

    assert_eq!(resolved, [ActivationAction::Focus(owner)]);
}

#[test]
fn imports_and_the_two_window_actions_pass_through_untouched() {
    // The same .textpack twice really does give two windows: an import is untitled, so nothing
    // owns it — the Mac's behaviour for the same Example opened twice.
    let batch = vec![
        ActivationAction::ImportTextPack("/a.textpack".into()),
        ActivationAction::ImportTextPack("/a.textpack".into()),
        ActivationAction::ImportTextBundleFolder("/b.textbundle".into()),
        ActivationAction::OpenUntitled,
        ActivationAction::RestoreSession,
    ];

    assert_eq!(
        activation_router::resolve(batch.clone(), &FakeDocumentRegistry::new(), &FakeFileIdentity::new()),
        batch
    );
}

// ── adversarial: the traps the porting reports name ─────────────────────────────────────────────

#[test]
fn a_second_instance_double_clicking_a_file_this_one_already_has_opens_no_window() {
    // The single-instance case end to end (§1.1, §1.2): the second process forwards over D-Bus,
    // this one is handed an `open` with is_first false, and the whole answer must be "come to the
    // front" — never a second window on the same file, and never an untitled one either.
    let registry = FakeDocumentRegistry::new();
    let identity = FakeFileIdentity::new();
    let owner = WindowId(9);
    registry.register(owner, &identity.canonical("/Docs/a.md"));

    let routed =
        route(&ActivationDescription::for_files(vec![ActivationItem::file("/Docs/../Docs//a.md")], false, false));
    let resolved = activation_router::resolve(routed, &registry, &identity);

    assert_eq!(resolved, [ActivationAction::Focus(owner)]);
}

#[test]
fn a_file_whose_window_closed_while_its_save_was_in_flight_opens_a_fresh_window() {
    // The window is gone but the write may still be draining. What must NOT happen is a Focus of a
    // window id that no longer exists: the manager would find nothing, and §1.2's "activate that
    // window instead" would silently open nothing at all.
    let registry = FakeDocumentRegistry::new();
    let identity = FakeFileIdentity::new();
    let closed = WindowId(10);
    registry.register(closed, &identity.canonical("/Docs/a.md"));
    registry.unregister(closed); // the window's close → cleanup (§1.4 route 3)

    let resolved = activation_router::resolve([open("/Docs/a.md")], &registry, &identity);

    assert_eq!(resolved, [open("/Docs/a.md")]);
}

/// md.win: NTFS may rename by case alone, so every case spelling must still find the open window.
/// LINUX: the other case spellings are other files and open as such; only the file's own spelling
/// (or one that canonicalises to it) is the open document.
#[test]
fn a_case_only_rename_is_still_the_same_open_document() {
    let registry = FakeDocumentRegistry::new();
    let identity = FakeFileIdentity::new();
    let owner = WindowId(11);
    registry.register(owner, &identity.canonical("/Docs/Notes.md"));

    for spelling in ["/Docs/Notes.md", "/Docs/./Notes.md", "//Docs/Notes.md"] {
        let resolved = activation_router::resolve([open(spelling)], &registry, &identity);
        assert_eq!(resolved, [ActivationAction::Focus(owner)], "{spelling}");
    }
    for spelling in ["/Docs/NOTES.MD", "/docs/notes.md"] {
        let resolved = activation_router::resolve([open(spelling)], &registry, &identity);
        assert_eq!(resolved, [open(spelling)], "{spelling}");
    }
}

#[test]
fn a_file_named_like_a_bundle_is_a_document_when_the_shell_says_it_is_a_file() {
    // §1.2's File row is explicit that the bundle import is for a FOLDER whose name ends in
    // .textbundle. A .textbundle that is a FILE is not a bundle at all — sending it to the folder
    // importer could only ever produce "The TextPack could not be read."
    let actions = route(&ActivationDescription::for_files(
        vec![ActivationItem::file("/Docs/notes.textbundle"), ActivationItem::folder("/Docs/real.textbundle")],
        true,
        false,
    ));

    assert_eq!(
        actions,
        [open("/Docs/notes.textbundle"), ActivationAction::ImportTextBundleFolder("/Docs/real.textbundle".into())]
    );

    // A command line has no file system to ask, so there the name is still taken at its word.
    assert_eq!(
        route(&ActivationDescription::for_launch(&["/Docs/real.textbundle"], true, false)),
        [ActivationAction::ImportTextBundleFolder("/Docs/real.textbundle".into())]
    );
}

#[test]
fn classify_ignores_a_blank_path() {
    assert_eq!(classify(&ActivationItem::file("   ")), None);
    assert_eq!(classify(&ActivationItem::folder("")), None);
}
