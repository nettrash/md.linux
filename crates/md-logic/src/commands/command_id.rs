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
/// - md.win's `Exit` is [`CommandId::Quit`]: the Mac's "Quit md" (its app menu row, ⌘Q) moved to the
///   end of File — where md.win put Exit — with GNOME's Ctrl+Q.
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
    /// Edit ▸ Replace…: the find bar with the replacement field focused (shell-design §3.5).
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

    /// Not a menu row: a window accelerator that is enabled only while Zen is on or the find bar is
    /// open (§2.9), so the editor keeps Esc otherwise.
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
    /// No menu row at all — the command exists only as an accelerator ([`CommandId::Escape`]).
    Accelerator,
}
