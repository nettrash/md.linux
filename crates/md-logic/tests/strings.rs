//! `strings`: the formatting functions, and the characters the escapes must produce. (md.win has
//! no test file of its own for `Strings`; the suites that use a string pin it in context. These
//! pin the functions' output against md.win's C# by hand.)

use md_logic::strings::{self, books, buttons, documents, exports, find, help, preview};

#[test]
fn the_escapes_are_the_typographic_characters() {
    assert_eq!(strings::ELLIPSIS, "…");
    assert_eq!(strings::EM_DASH, "—");
    assert_eq!(strings::EDITED_SUFFIX, " — Edited");
    assert_eq!(strings::EDITOR_PLACEHOLDER, "# Start writing…");
    assert_eq!(buttons::SAVE_AS, "Save As…");
    assert_eq!(help::COPYRIGHT, "© 2026 nettrash. MIT licensed.");
    assert_eq!(exports::DIAGRAM_NOT_CAPTURED, "This diagram couldn't be captured — it may have failed to render.");
    assert_eq!(
        books::NEW_CHAPTER_MESSAGE,
        "A chapter is a folder. Start the name with a number (\"02 …\") to place it in the reading order."
    );
}

#[test]
fn untitled_windows_are_numbered_from_two() {
    assert_eq!(strings::untitled_numbered(0), "Untitled");
    assert_eq!(strings::untitled_numbered(1), "Untitled");
    assert_eq!(strings::untitled_numbered(2), "Untitled 2");
    assert_eq!(strings::untitled_numbered(12), "Untitled 12");
}

#[test]
fn the_title_and_footer_helpers() {
    assert_eq!(strings::duplicate_title("Notes"), "Notes copy");
    assert_eq!(strings::footer(1, 1), "1 words · 1 characters"); // no pluralisation, U+00B7
    assert_eq!(strings::footer(1234, 56789), "1234 words · 56789 characters"); // no group separators
}

#[test]
fn the_document_session_messages() {
    assert_eq!(documents::could_not_save("disk full"), "Couldn’t save — disk full");
    assert_eq!(
        documents::save_changes_question("Untitled"),
        "Do you want to save the changes made to the document “Untitled”?"
    );
    assert_eq!(documents::rescue_copy_name("a", ".md", 1), "a (rescued).md");
    assert_eq!(documents::rescue_copy_name("a", ".md", 2), "a (rescued 2).md");
    assert_eq!(documents::rescue_copy_name("README", "", 100), "README (rescued 100)");
    assert_eq!(documents::rescue_failed_title("a.md"), "Could not save “a.md”");
    assert_eq!(
        documents::rescue_kept_message("a (rescued).md"),
        "Your text was kept as “a (rescued).md” in the same folder."
    );
    assert_eq!(documents::file_not_found("a.md"), "The file “a.md” could not be found.");
}

#[test]
fn the_rename_rule_is_the_macs_and_md_cores() {
    // LINUX DECISION (see strings): the Mac's sentence, matching md-core's is_valid_name.
    assert_eq!(documents::INVALID_NAME_MESSAGE, "A name cannot contain \"/\" or \":\".");
    use md_core::book::book_folder::is_valid_name;
    assert!(!is_valid_name("a/b"));
    assert!(!is_valid_name("a:b"));
    assert!(is_valid_name("a?b")); // Win32 refuses it; the message no longer names it
}

#[test]
fn the_book_and_export_messages() {
    assert_eq!(exports::folder_exists("x.textbundle"), "A folder named “x.textbundle” already exists.");
    assert_eq!(books::delete_question("01 Intro"), "Delete “01 Intro”?");
    assert_eq!(
        books::handoff_message("Intro"),
        "“Intro” is open as a document window, and that window owns the file while it stays open. Close it to write here again."
    );
    assert_eq!(
        books::unreadable_message("Intro"),
        "“Intro” could not be read. It may have been moved or deleted outside the book."
    );
    assert_eq!(books::article_unreadable("a.md"), "The article \"a.md\" could not be read.");
}

#[test]
fn the_find_tally() {
    assert_eq!(find::tally(0, 0, false), "");
    assert_eq!(find::tally(3, 12, false), "");
    assert_eq!(find::tally(0, 0, true), find::NO_RESULTS);
    assert_eq!(find::tally(3, 12, true), "3 of 12");
    assert_eq!(find::tally(0, 1, true), "1 result");
    assert_eq!(find::tally(0, 12, true), "12 results");
    assert_eq!(find::tally(1, 1, true), "1 of 1");
}

#[test]
fn help_points_at_the_linux_pages() {
    assert_eq!(help::SUPPORT_URL, "https://nettrash.me/linux/md/support.html");
    assert_eq!(help::PRIVACY_URL, "https://nettrash.me/linux/md/privacy.html");
    assert_eq!(help::ENGINES.len(), 5);
}

// ── LINUX: the strings the shell would otherwise spell (shell-design §11, §15) ────────────

#[test]
fn the_preview_give_up_line_is_the_macs_word_for_word() {
    // md.macOS PreviewRetryPolicy.noticeSource; U+2014, one emphasised line of Markdown.
    assert_eq!(
        preview::STOPPED_NOTICE_SOURCE,
        "*The preview stopped unexpectedly. Your document is unchanged \u{2014} the next edit will try again.*"
    );
    assert!(!preview::STOPPED_NOTICE_SOURCE.contains('\n'));
    // Rendered through the ordinary document pipeline, it is one paragraph of emphasis.
    let html = md_core::markdown::html::document(preview::STOPPED_NOTICE_SOURCE, "notes", false, false);
    assert!(html.contains(
        "<p><em>The preview stopped unexpectedly. Your document is unchanged \u{2014} the next edit will try again.</em></p>"
    ));
}

#[test]
fn the_open_dialogs_filter_is_named() {
    assert_eq!(documents::OPEN_FILTER, "Supported Documents");
}
