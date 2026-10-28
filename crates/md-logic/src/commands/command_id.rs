//! Port of md.win `Commands/CommandId.cs`: every command the shell defines, and what the menu
//! builder makes of each.

/// Every command the shell defines (shell-design §2, §13.2). One id per row of the menu tables —
/// including the rows that expand into many menu items at run time ([`CommandId::OpenRecentEntry`],
/// [`CommandId::Example`], [`CommandId::Contents`], [`CommandId::Notes`],
/// [`CommandId::ExportDiagramSvg`], [`CommandId::PdfPageSize`], [`CommandId::ActivateWindow`]):
/// the id names the *command*, the [`CommandArgument`](super::CommandArgument) passed to
/// [`CommandDispatcher::try_invoke`](super::CommandDispatcher::try_invoke) names which row. Ids are
/// never persisted; the names are the contract every module codes against.
///
/// LINUX DECISIONS against md.win's enum:
/// - md.win's `ShareSource`, `ShareRenderedPdf` and `ShareBookPdf` are gone: Linux has no system
///   share sheet (see `seams`), so File ▸ Share ▸ and Book ▸ Share Book as PDF do not exist. Export
///   ▸ PDF… and Export Book ▸ PDF… write the same files.
/// - md.win's `Exit` is [`CommandId::Quit`]: the Mac's app-menu row (⌘Q) moved to the end of File
///   — where md.win put Exit — titled "Quit", GNOME's word, with GNOME's Ctrl+Q (shell-design
///   §2.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CommandId {
    // File (§2.2)
    New,
    Open,
    OpenRecentEntry,
    ClearRecent,
    OpenTextBundleFolder,
    Example,
    ExampleBook,
    Close,
    Save,
    SaveAs,
    Duplicate,
    Rename,
    MoveTo,
    RevertToSaved,
    Print,
    ExportPdf,
    ExportHtml,
    ExportEpub,
    ExportLaTeX,
    ExportTextBundle,
    ExportDiagramSvg,
    PdfPageSize,
    Quit,

    // Edit (§2.4)
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Find,
    FindNext,
    FindPrevious,
    /// Edit ▸ Find ▸ Find and Replace…: the find bar with the replacement row shown and its field
    /// focused (shell-design §3.6).
    Replace,
    UseSelectionForFind,
    /// Edit ▸ Typing ▸ Continue Lists and Tables: the `md.continueLists` toggle (smart-typing.md
    /// §3.1).
    ContinueLists,
    /// Edit ▸ Typing ▸ Capitalize Sentences: the `md.capitalizeSentences` toggle.
    CapitalizeSentences,
    /// Edit ▸ Typing ▸ Wrap Selection When Typing * _ ` $: the `md.wrapSelection` toggle
    /// (formatting.md §4).
    WrapSelection,

    // Format (1.6, formatting.md) — each one undoable edit of the editor's text.
    Bold,
    Italic,
    InlineCode,
    Strikethrough,
    Link,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    BulletList,
    TaskList,
    Quote,
    Indent,
    Outdent,

    // View (§2.5)
    ViewEdit,
    ViewSplit,
    ViewPreview,
    ZenMode,
    ShowSidebar,
    FullScreen,

    // Book (§2.6)
    NewBook,
    OpenBook,
    ShowBook,
    CloseBook,
    PrintBook,
    ExportBookPdf,
    ExportBookEpub,
    ExportBookLaTeX,

    // Go (§2.7)
    PreviousArticle,
    NextArticle,
    Contents,
    Notes,

    // Window and Help (§2.8)
    Minimize,
    Zoom,
    ActivateWindow,
    Help,
    PrivacyPolicy,
    About,

    /// Not a menu row and not an accelerator: the Escape key as the window's own key controller
    /// sees it in the bubble phase, after everything below it declined the key (shell-design
    /// §2.2). Enabled only while Zen is on or the find bar is open, so the key is otherwise left
    /// alone.
    Escape,
}

impl CommandId {
    /// Every id, in declaration order — C#'s `Enum.GetValues<CommandId>()`.
    pub const ALL: [CommandId; 79] = [
        CommandId::New,
        CommandId::Open,
        CommandId::OpenRecentEntry,
        CommandId::ClearRecent,
        CommandId::OpenTextBundleFolder,
        CommandId::Example,
        CommandId::ExampleBook,
        CommandId::Close,
        CommandId::Save,
        CommandId::SaveAs,
        CommandId::Duplicate,
        CommandId::Rename,
        CommandId::MoveTo,
        CommandId::RevertToSaved,
        CommandId::Print,
        CommandId::ExportPdf,
        CommandId::ExportHtml,
        CommandId::ExportEpub,
        CommandId::ExportLaTeX,
        CommandId::ExportTextBundle,
        CommandId::ExportDiagramSvg,
        CommandId::PdfPageSize,
        CommandId::Quit,
        CommandId::Undo,
        CommandId::Redo,
        CommandId::Cut,
        CommandId::Copy,
        CommandId::Paste,
        CommandId::Delete,
        CommandId::SelectAll,
        CommandId::Find,
        CommandId::FindNext,
        CommandId::FindPrevious,
        CommandId::Replace,
        CommandId::UseSelectionForFind,
        CommandId::ContinueLists,
        CommandId::CapitalizeSentences,
        CommandId::WrapSelection,
        CommandId::Bold,
        CommandId::Italic,
        CommandId::InlineCode,
        CommandId::Strikethrough,
        CommandId::Link,
        CommandId::Heading1,
        CommandId::Heading2,
        CommandId::Heading3,
        CommandId::Heading4,
        CommandId::Heading5,
        CommandId::Heading6,
        CommandId::BulletList,
        CommandId::TaskList,
        CommandId::Quote,
        CommandId::Indent,
        CommandId::Outdent,
        CommandId::ViewEdit,
        CommandId::ViewSplit,
        CommandId::ViewPreview,
        CommandId::ZenMode,
        CommandId::ShowSidebar,
        CommandId::FullScreen,
        CommandId::NewBook,
        CommandId::OpenBook,
        CommandId::ShowBook,
        CommandId::CloseBook,
        CommandId::PrintBook,
        CommandId::ExportBookPdf,
        CommandId::ExportBookEpub,
        CommandId::ExportBookLaTeX,
        CommandId::PreviousArticle,
        CommandId::NextArticle,
        CommandId::Contents,
        CommandId::Notes,
        CommandId::Minimize,
        CommandId::Zoom,
        CommandId::ActivateWindow,
        CommandId::Help,
        CommandId::PrivacyPolicy,
        CommandId::About,
        CommandId::Escape,
    ];

    /// The `GAction` name the GTK shell gives this command (without the `app.` / `win.` prefix):
    /// the id in kebab case, so `set_accels_for_action("win.save-as", …)` and the menu model's
    /// `action` attribute are spelled in one place. Valid for `g_action_name_is_valid` (ASCII
    /// letters, digits, `-`), unique across the table.
    pub fn action_name(self) -> String {
        // `{:?}` is the variant name, ASCII only: every capital after the first starts a word
        // (`SaveAs` → `save-as`, `Heading1` → `heading1`), and the one mixed-case word, LaTeX, is
        // put back together (`ExportLaTeX` → `export-latex`).
        let mut name = String::new();
        for (i, c) in format!("{self:?}").chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                name.push('-');
            }
            name.push(c.to_ascii_lowercase());
        }
        name.replace("la-te-x", "latex")
    }

    /// Which action group the GTK shell puts this command in (shell-design §2.2): `app.` for the
    /// commands whose state is the application's — opening things, the recent list, the examples,
    /// Quit, the typing and page-size settings, the Book menu and Help — and `win.` for everything
    /// that acts on one window. Exhaustive, so a new id cannot be left without a scope.
    pub fn scope(self) -> ActionScope {
        use CommandId as C;
        match self {
            C::New
            | C::Open
            | C::OpenRecentEntry
            | C::ClearRecent
            | C::OpenTextBundleFolder
            | C::Example
            | C::ExampleBook
            | C::Quit
            | C::ContinueLists
            | C::CapitalizeSentences
            | C::WrapSelection
            | C::PdfPageSize
            | C::NewBook
            | C::OpenBook
            | C::ShowBook
            | C::CloseBook
            | C::PrintBook
            | C::ExportBookPdf
            | C::ExportBookEpub
            | C::ExportBookLaTeX
            | C::Help
            | C::PrivacyPolicy
            | C::About => ActionScope::App,
            C::Close
            | C::Save
            | C::SaveAs
            | C::Duplicate
            | C::Rename
            | C::MoveTo
            | C::RevertToSaved
            | C::Print
            | C::ExportPdf
            | C::ExportHtml
            | C::ExportEpub
            | C::ExportLaTeX
            | C::ExportTextBundle
            | C::ExportDiagramSvg
            | C::Undo
            | C::Redo
            | C::Cut
            | C::Copy
            | C::Paste
            | C::Delete
            | C::SelectAll
            | C::Find
            | C::FindNext
            | C::FindPrevious
            | C::Replace
            | C::UseSelectionForFind
            | C::Bold
            | C::Italic
            | C::InlineCode
            | C::Strikethrough
            | C::Link
            | C::Heading1
            | C::Heading2
            | C::Heading3
            | C::Heading4
            | C::Heading5
            | C::Heading6
            | C::BulletList
            | C::TaskList
            | C::Quote
            | C::Indent
            | C::Outdent
            | C::ViewEdit
            | C::ViewSplit
            | C::ViewPreview
            | C::ZenMode
            | C::ShowSidebar
            | C::FullScreen
            | C::PreviousArticle
            | C::NextArticle
            | C::Contents
            | C::Notes
            | C::Minimize
            | C::Zoom
            | C::ActivateWindow
            | C::Escape => ActionScope::Window,
        }
    }
}

/// The two `GActionGroup`s a command can live in (shell-design §2.2). While an alert is up in a
/// window, every [`ActionScope::Window`] command of that window is disabled
/// ([`ShellSnapshot::dialog_open`](super::ShellSnapshot::dialog_open)); the application's own
/// commands stay live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ActionScope {
    /// `app.` — one action for the whole process (`gtk::Application`'s action map).
    App,
    /// `win.` — one action per window (`adw::ApplicationWindow`'s action map).
    Window,
}

impl ActionScope {
    /// The prefix of a detailed action name: `"app"` or `"win"`.
    pub fn prefix(self) -> &'static str {
        match self {
            ActionScope::App => "app",
            ActionScope::Window => "win",
        }
    }
}

/// What the menu builder makes of a [`CommandSpec`](super::CommandSpec). The GTK shapes are
/// named for orientation only — md-logic never references them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandKind {
    /// A plain `GMenuItem` on a stateless action.
    Item,
    /// A check item: a stateful boolean action, ticked from
    /// [`command_enablement::is_checked`](super::command_enablement::is_checked).
    Toggle,
    /// One item per snapshot row (Open Recent, Examples, Contents, Notes, Diagram as SVG), each
    /// with its own target.
    DynamicItems,
    /// One item per snapshot row, ticked on the current one (the Window list).
    DynamicToggles,
    /// One radio item per snapshot row (PDF Page Size): a stateful string action whose state is the
    /// ticked row's target.
    DynamicRadios,
    /// No menu row and no registered accelerator: a key the window's own `gtk::EventControllerKey`
    /// dispatches in the bubble phase, once nothing below it handled the key
    /// ([`CommandId::Escape`], shell-design §2.2). Registering Escape with
    /// `set_accels_for_action` would take it from input methods, popovers and the find field.
    WindowKey,
}
