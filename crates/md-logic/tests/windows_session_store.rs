//! Port of md.win `tests/Md.App.Logic.Tests/SessionStoreTests.cs`.
//!
//! `session.json` — the codec, the disk round trip and the three rules of §1.6. md.win's folder is
//! `LocalFolder`; here it is the XDG data folder the app passes in.

use std::io;

use md_core::document::view_mode::ViewMode;
use md_logic::settings::settings_keys;
use md_logic::testing::*;
use md_logic::windows::{SessionBook, SessionState, SessionWindow, WindowRect, session_store};

const FOLDER: &str = "/home/n/.local/share/me.nettrash.md";

fn row(path: &str) -> SessionWindow {
    row_with(path, ViewMode::Split, false, false)
}

fn row_with(path: &str, mode: ViewMode, zen: bool, zen_reading: bool) -> SessionWindow {
    SessionWindow::new(path, mode, zen, zen_reading, WindowRect::new(120, 80, 900, 640), false)
}

#[test]
fn the_file_name_is_the_one_settings_keys_publishes_for_privacy() {
    assert_eq!(session_store::FILE_NAME, "session.json");
    assert_eq!(session_store::FILE_NAME, settings_keys::SESSION_FILE_NAME);
    assert_eq!(session_store::path_in(FOLDER), "/home/n/.local/share/me.nettrash.md/session.json");
}

#[test]
fn the_document_shape_is_the_designs() {
    let state = SessionState::new(
        vec![SessionWindow::new(
            "/Docs/a.md",
            ViewMode::Split,
            false,
            false,
            WindowRect::new(120, 80, 900, 640),
            false,
        )],
        Some(SessionBook::new(true, WindowRect::new(200, 100, 1000, 700), false)),
    );

    assert_eq!(
        session_store::encode(&state),
        r#"{"v":1,"windows":[{"path":"/Docs/a.md","mode":"split","zen":false,"zenReading":false,"x":120,"y":80,"w":900,"h":640,"maximized":false}],"book":{"open":true,"x":200,"y":100,"w":1000,"h":700,"maximized":false}}"#
    );
}

/// LINUX: the escapes JSON requires and nothing else — a quote and a backslash in a name (both
/// legal on Linux), and non-ASCII as itself, where md.win's writer spells it `\uXXXX`.
#[test]
fn a_path_is_written_with_only_the_escapes_json_requires() {
    let path = "/Docs/\"quoted\" back\\slash Привет.md";
    let json = session_store::encode(&SessionState::new(vec![row(path)], None));
    assert!(json.contains(r#""path":"/Docs/\"quoted\" back\\slash Привет.md""#), "{json}");
    assert_eq!(session_store::decode(Some(&json)).windows[0].path, path);
}

#[test]
fn every_window_field_survives_the_round_trip() {
    for mode in [ViewMode::Edit, ViewMode::Split, ViewMode::Preview] {
        let state = SessionState::new(
            vec![SessionWindow::new("/Docs/a.md", mode, true, true, WindowRect::new(-40, 7, 1024, 768), true)],
            Some(SessionBook::new(false, WindowRect::new(1, 2, 3, 4), true)),
        );

        assert_eq!(session_store::decode(Some(&session_store::encode(&state))), state, "{mode:?}");
    }
}

#[test]
fn a_session_with_no_book_window_omits_the_book_object() {
    let json = session_store::encode(&SessionState::new(vec![row("/a.md")], None));
    assert!(!json.contains("\"book\""));
    assert_eq!(session_store::decode(Some(&json)).book, None);
}

#[test]
fn an_untitled_document_is_never_recorded() {
    // §1.6: only saved documents are listed — an untitled draft is not autosaved (§14 row 8), so a
    // row for one would restore an empty window over text the writer never got back.
    let state = SessionState::new(vec![row(""), row("   "), row("/Docs/a.md")], None);
    let decoded = session_store::decode(Some(&session_store::encode(&state)));

    assert_eq!(decoded.windows.len(), 1);
    assert_eq!(decoded.windows[0].path, "/Docs/a.md");
}

#[test]
fn the_encoder_itself_omits_the_pathless_row_rather_than_leaving_it_for_the_decoder() {
    // The round trip above passes either way: decode drops a pathless row too, so it cannot tell
    // whether encode wrote one. §1.6 is a property of the FILE — PRIVACY.md enumerates what
    // session.json holds — so the assertion has to be on the bytes.
    let json = session_store::encode(&SessionState::new(vec![row(""), row("   "), row("/Docs/a.md")], None));

    assert!(!json.contains(r#""path":"""#));
    assert!(!json.contains(r#""path":"   ""#));
    assert_eq!(json.matches(r#""path":"#).count(), 1);
}

#[test]
fn a_session_written_by_a_later_version_is_ignored_whole_rather_than_read_in_part() {
    // The forward-compatibility trap: a v2 file whose rows still LOOK like v1 rows must not be
    // half-understood — the fields a later md adds are exactly the ones this build would drop on
    // the next write, silently losing them.
    let later = r#"{"v":2,"windows":[{"path":"/Docs/a.md","mode":"edit","zen":true,"x":1,"y":2,"w":900,"h":640,"pinned":true}],"book":{"open":true,"x":0,"y":0,"w":1,"h":1}}"#;
    assert_eq!(session_store::decode(Some(later)), SessionState::EMPTY);

    // And the other direction: a v1 file carrying fields this build has never heard of still
    // reads, because the version is the contract and unknown keys are not.
    let v1_with_extras = r#"{"v":1,"theme":"dark","windows":[{"path":"/Docs/a.md","mode":"edit","zen":true,"zenReading":false,"x":1,"y":2,"w":900,"h":640,"maximized":false,"pinned":true}]}"#;
    let decoded = session_store::decode(Some(v1_with_extras));
    assert_eq!(decoded.windows.len(), 1);
    let row = &decoded.windows[0];
    assert_eq!(row.path, "/Docs/a.md");
    assert_eq!(row.mode, ViewMode::Edit);
    assert!(row.zen);
    assert_eq!(row.placement, WindowRect::new(1, 2, 900, 640));
}

#[test]
fn an_unreadable_session_is_an_empty_one() {
    let rows = [
        None,
        Some(""),
        Some("   "),
        Some("not json"),
        Some("[]"),
        Some("{}"),
        Some(r#"{"v":2,"windows":[{"path":"/a.md"}]}"#),
        Some(r#"{"v":1,"windows":[{"path":"/a.md"}"#),
        // Not JSON as System.Text.Json reads it either: a trailing comma, a comment, "v" not an
        // integer.
        Some(r#"{"v":1,"windows":[{"path":"/a.md"},]}"#),
        Some(r#"{"v":1 /* c */,"windows":[]}"#),
        Some(r#"{"v":1.0,"windows":[{"path":"/a.md"}]}"#),
        Some(r#"{"v":"1","windows":[{"path":"/a.md"}]}"#),
    ];
    for json in rows {
        assert_eq!(session_store::decode(json), SessionState::EMPTY, "{json:?}");
    }
}

#[test]
fn missing_fields_take_their_defaults_rather_than_dropping_the_row() {
    let decoded = session_store::decode(Some(r#"{"v":1,"windows":[{"path":"/a.md"}]}"#));
    assert_eq!(decoded.windows.len(), 1);
    let row = &decoded.windows[0];

    assert_eq!(row.mode, ViewMode::WINDOW_DEFAULT);
    assert!(!row.zen);
    assert!(!row.zen_reading);
    assert_eq!(row.placement, WindowRect::new(0, 0, 0, 0));
    assert!(!row.maximized);
}

#[test]
fn an_unknown_mode_reads_as_the_window_default() {
    let decoded = session_store::decode(Some(r#"{"v":1,"windows":[{"path":"/a.md","mode":"zen"}]}"#));
    assert_eq!(decoded.windows.len(), 1);
    assert_eq!(decoded.windows[0].mode, ViewMode::WINDOW_DEFAULT);
}

#[test]
fn restoring_skips_files_that_are_gone() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/a.md", "# a");
    let state = SessionState::new(
        vec![row("/Docs/a.md"), row("/Docs/gone.md")],
        Some(SessionBook::new(true, WindowRect::new(0, 0, 1000, 700), false)),
    );

    let restorable = session_store::restorable(&state, &fs);

    assert_eq!(restorable.windows.len(), 1);
    assert_eq!(restorable.windows[0].path, "/Docs/a.md");
    assert!(restorable.book.is_some());
}

#[test]
fn a_book_that_was_not_open_is_not_restored() {
    let state = SessionState::new(Vec::new(), Some(SessionBook::new(false, WindowRect::new(0, 0, 1000, 700), false)));
    assert_eq!(session_store::restorable(&state, &FakeFileSystem::new()).book, None);
}

#[test]
fn the_disk_round_trip_goes_through_local_folder_session_json() {
    let fs = FakeFileSystem::new();
    fs.add_directory(FOLDER);
    fs.add_file("/Docs/a.md", "# a");
    let state = SessionState::new(vec![row_with("/Docs/a.md", ViewMode::Preview, true, true)], None);

    assert!(session_store::save(&fs, FOLDER, &state));
    assert_eq!(session_store::load(&fs, FOLDER), state);
    assert_eq!(session_store::restorable(&session_store::load(&fs, FOLDER), &fs).windows, state.windows);
}

#[test]
fn a_missing_session_file_loads_as_empty() {
    assert_eq!(session_store::load(&FakeFileSystem::new(), FOLDER), SessionState::EMPTY);
}

#[test]
fn a_failed_write_is_reported_rather_than_thrown_so_a_close_is_never_blocked() {
    let fs = FakeFileSystem::new();
    fs.add_directory(FOLDER);
    fs.set_next_write_failure(Some(io::Error::new(io::ErrorKind::PermissionDenied, "read-only")));
    assert!(!session_store::save(&fs, FOLDER, &SessionState::new(vec![row("/a.md")], None)));
}

/// LINUX: the XDG folder does not exist on a first run (md.win's `LocalFolder` always does); a save
/// into it fails quietly until the app creates it.
#[test]
fn a_save_into_a_folder_that_does_not_exist_yet_reports_false() {
    let fs = FakeFileSystem::new();
    assert!(!session_store::save(&fs, FOLDER, &SessionState::new(vec![row("/a.md")], None)));
    fs.add_directory(FOLDER);
    assert!(session_store::save(&fs, FOLDER, &SessionState::new(vec![row("/a.md")], None)));
}
